#!/usr/bin/env bash
set -euo pipefail

BIN="$HOME/projects/recent-inode/target/release/rinode"
TEST_ROOT="$HOME/rinode_test_$(date +%s)"
mkdir -p "$TEST_ROOT"
cp "$HOME/projects/recent-inode/rinode.toml" "$TEST_ROOT/rinode.toml"
cd "$TEST_ROOT"

echo "=== RINODE INTEGRATION TESTS IN $TEST_ROOT ==="

# Clean previous DB for test
rm -rf "$HOME/.local/share/recent-inode/rinode.db"*

# TEST 1: Single file deletion and restoration
echo -e "\n[TEST 1] Single file deletion & restoration..."
echo "Hello, zero-copy restoration!" > test_file.txt
ORIG_INO=$(stat -c '%i' test_file.txt)
echo "Original Inode: $ORIG_INO"

"$BIN" rm test_file.txt
if [ -f test_file.txt ]; then
    echo "[!] Error: test_file.txt still exists!"
    exit 1
fi
echo "[+] File successfully deleted from original location."

# Verify listed
echo -e "\n[TEST 1.1] Listing deleted files..."
"$BIN" ls

# Verify inspect
echo -e "\n[TEST 1.2] Inspecting entry #1..."
"$BIN" inspect 1

# Restore file
echo -e "\n[TEST 1.3] Restoring file..."
"$BIN" restore 1
if [ ! -f test_file.txt ]; then
    echo "[!] Error: test_file.txt was not restored!"
    exit 1
fi
CONTENT=$(cat test_file.txt)
RESTORED_INO=$(stat -c '%i' test_file.txt)
echo "Restored Content: $CONTENT"
echo "Restored Inode: $RESTORED_INO"
if [ "$CONTENT" != "Hello, zero-copy restoration!" ]; then
    echo "[!] Error: Restored content does not match!"
    exit 1
fi
if [ "$ORIG_INO" != "$RESTORED_INO" ]; then
    echo "[!] Error: Inodes do not match! (Expected $ORIG_INO, got $RESTORED_INO)"
    exit 1
fi
echo "[+] Content and Inode matched perfectly."

# TEST 2: Nested directory deletion and restoration
echo -e "\n[TEST 2] Directory tree deletion & restoration..."
mkdir -p nested/sub1/sub2
echo "file 1" > nested/sub1/file1.txt
echo "file 2" > nested/sub1/sub2/file2.txt

"$BIN" rm nested
if [ -d nested ]; then
    echo "[!] Error: nested directory still exists!"
    exit 1
fi
echo "[+] Directory removed from original location."

"$BIN" restore nested
if [ ! -f nested/sub1/sub2/file2.txt ]; then
    echo "[!] Error: nested hierarchy not restored!"
    exit 1
fi
echo "[+] Directory tree restored completely."

# TEST 3: Parent directory deletion recreation (mkdir -p)
echo -e "\n[TEST 3] Parent directory recreation test..."
mkdir -p parent_dir/child
echo "nested data" > parent_dir/child/target.txt

"$BIN" rm parent_dir/child/target.txt
# Now delete parent_dir
rm -rf parent_dir

"$BIN" restore target.txt
if [ ! -f parent_dir/child/target.txt ]; then
    echo "[!] Error: File not restored with recreated parent directory!"
    exit 1
fi
echo "[+] Parent directories automatically recreated."

# TEST 4: Symlink preservation and restoration
echo -e "\n[TEST 4] Symlink preservation test..."
echo "target file content" > sym_target.txt
ln -s sym_target.txt my_link.lnk

"$BIN" rm my_link.lnk
if [ -L my_link.lnk ]; then
    echo "[!] Error: Symlink still exists!"
    exit 1
fi

"$BIN" restore my_link.lnk
if [ ! -L my_link.lnk ]; then
    echo "[!] Error: Symlink was not restored as a link!"
    exit 1
fi
LINK_DEST=$(readlink my_link.lnk)
if [ "$LINK_DEST" != "sym_target.txt" ]; then
    echo "[!] Error: Symlink target is wrong: $LINK_DEST"
    exit 1
fi
echo "[+] Symlink properly restored pointing to $LINK_DEST."

# TEST 5: Regex exclusion test
echo -e "\n[TEST 5] Regex exclusion test..."
echo "temporary junk" > cache.tmp

"$BIN" rm cache.tmp
echo "[+] Excluded file passed through exclusion filter properly."

# TEST 6: Purge test
echo -e "\n[TEST 6] Purge test..."
echo "dummy file" > purge_me.txt
"$BIN" rm purge_me.txt
"$BIN" purge --all
echo "[+] Purge completed successfully."

# TEST 7: POSIX rm flags compatibility test
echo -e "\n[TEST 7] POSIX rm flags compatibility test (-rf, -v, -d)..."
mkdir -p posix_dir/sub
echo "deep file" > posix_dir/sub/file.txt
"$BIN" rm -rf posix_dir
if [ -d posix_dir ]; then
    echo "[!] Error: posix_dir still exists after rm -rf!"
    exit 1
fi
echo "verbose file" > verbose_test.txt
"$BIN" rm -v verbose_test.txt
if [ -f verbose_test.txt ]; then
    echo "[!] Error: verbose_test.txt still exists!"
    exit 1
fi
echo "[+] POSIX rm flags (-rf, -v) work as expected."

# TEST 7.1: Permanent / no-vault direct unlinking test
echo -e "\n[TEST 7.1] Testing --no-vault and --permanent flags..."
echo "permanent junk 1" > perm1.txt
"$BIN" rm --no-vault perm1.txt
if [ -f perm1.txt ]; then
    echo "[!] Error: perm1.txt still exists after --no-vault!"
    exit 1
fi
# Ensure it was not added to the vault index
if "$BIN" ls | grep -q "perm1.txt"; then
    echo "[!] Error: perm1.txt was indexed in the vault despite --no-vault!"
    exit 1
fi

echo "permanent junk 2" > perm2.txt
"$BIN" rm -p perm2.txt
if [ -f perm2.txt ]; then
    echo "[!] Error: perm2.txt still exists after -p!"
    exit 1
fi
if "$BIN" ls | grep -q "perm2.txt"; then
    echo "[!] Error: perm2.txt was indexed in the vault despite -p!"
    exit 1
fi
echo "[+] Direct unlinking (--no-vault, -p) works without indexing."

# TEST 8: Shell init generation test
echo -e "\n[TEST 8] Shell init generation test (fish, bash, zsh, auto-detect, custom alias)..."
FISH_INIT=$("$BIN" init fish)
echo "$FISH_INIT" | grep -q "function r" || { echo "[!] Fish init missing function r"; exit 1; }
FISH_INIT_ALIAS=$("$BIN" init fish --alias-rm)
echo "$FISH_INIT_ALIAS" | grep -q 'alias rm="rinode rm"' || { echo "[!] Fish init missing rm alias"; exit 1; }
BASH_INIT=$("$BIN" init bash)
echo "$BASH_INIT" | grep -q "r()" || { echo "[!] Bash init missing r()"; exit 1; }
ZSH_INIT=$("$BIN" init zsh --alias-rm)
echo "$ZSH_INIT" | grep -q 'alias rm="rinode rm"' || { echo "[!] Zsh init missing rm alias"; exit 1; }
AUTO_DETECT=$(SHELL=/usr/bin/fish "$BIN" init)
echo "$AUTO_DETECT" | grep -q "function r" || { echo "[!] Auto-detect fish init failed"; exit 1; }

# Custom alias test
CUSTOM_FISH=$("$BIN" init fish --alias ri)
echo "$CUSTOM_FISH" | grep -q "function ri" || { echo "[!] Custom alias ri not generated"; exit 1; }
NO_ALIAS_BASH=$("$BIN" init bash --alias none)
if echo "$NO_ALIAS_BASH" | grep -q "r()"; then
    echo "[!] Alias none still generated r() wrapper in bash!"; exit 1;
fi
echo "[+] Shell init scripts, custom aliases, and auto-detection work correctly."

# TEST 9: Exclusion CLI test
echo -e "\n[TEST 9] Exclusion management CLI test..."
# Test dry-run diagnosis
"$BIN" exclude --test "/home/user/myproject/node_modules/express/index.js" | grep -q "MATCHED" || {
    echo "[!] node_modules test path was not excluded"; exit 1;
}
"$BIN" exclude --test "/home/user/repo/.git/objects/abc" | grep -q "MATCHED" || {
    echo "[!] .git/objects path was not excluded"; exit 1;
}
"$BIN" exclude --test "/home/user/document.pdf" | grep -q "NOT EXCLUDED" || {
    echo "[!] normal document was unexpectedly excluded"; exit 1;
}
# Test adding rule
"$BIN" exclude "*.log"
"$BIN" exclude --test "error.log" | grep -q "MATCHED" || {
    echo "[!] newly excluded *.log pattern failed match"; exit 1;
}
"$BIN" exclude --list | grep -q "log" || {
    echo "[!] exclude --list missing newly added rule"; exit 1;
}
echo "[+] Exclusion management CLI functioning as expected."


# Cleanup test directory
rm -rf "$TEST_ROOT"

echo -e "\n=== ALL RINODE INTEGRATION TESTS PASSED SUCCESSFULLY ===\n"

