#!/usr/bin/env bash
set -euo pipefail

BIN="$HOME/projects/recent-inode/target/release/rinode"
TEST_ROOT="$HOME/rinode_test_$(date +%s)"
mkdir -p "$TEST_ROOT"
cp "$HOME/projects/recent-inode/rinode.toml" "$TEST_ROOT/rinode.toml"
cd "$TEST_ROOT"

# Isolate test database, vault, and config completely from host system
export XDG_DATA_HOME="$TEST_ROOT/data"
export XDG_CONFIG_HOME="$TEST_ROOT/config"
mkdir -p "$XDG_DATA_HOME" "$XDG_CONFIG_HOME"

echo "=== RINODE INTEGRATION TESTS IN $TEST_ROOT ==="

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

# TEST 1.4: Restore with --keep-copy
echo -e "\n[TEST 1.4] Restoring with --keep-copy..."
echo "snapshot copy test" > snap_test.txt
"$BIN" rm snap_test.txt
"$BIN" restore --keep-copy snap_test.txt
if [ ! -f snap_test.txt ]; then
    echo "[!] Error: snap_test.txt was not restored with --keep-copy!"
    exit 1
fi
INSPECT_LOC=$("$BIN" inspect 2 | grep "Storage Location:" | awk '{print $NF}')
if [ ! -f "$INSPECT_LOC" ]; then
    echo "[!] Error: Snapshot file not retained in storage on disk!"
    exit 1
fi
if ! "$BIN" ls -a | grep -q "snap_test.txt"; then
    echo "[!] Error: snap_test.txt not visible in ls -a after --keep-copy!"
    exit 1
fi
"$BIN" purge 2
echo "[+] Snapshot copy retained and restored correctly."

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
if [ -f cache.tmp ]; then
    echo "[!] Error: cache.tmp was not removed!"
    exit 1
fi

# Ensure it was tracked with EXCLUDED status in history
if ! "$BIN" ls -a | grep "cache.tmp" | grep -q "EXCLUDED"; then
    echo "[!] Error: cache.tmp not found in history with status EXCLUDED!"
    exit 1
fi

# Ensure trying to restore it returns an error
if "$BIN" restore cache.tmp 2>/dev/null; then
    echo "[!] Error: Restoring an EXCLUDED file should have failed!"
    exit 1
fi

echo "[+] Excluded file passed through exclusion filter properly and tracked in history."

# TEST 6: Purge test (all)
echo -e "\n[TEST 6] Purge test (all)..."
echo "dummy file" > purge_me.txt
"$BIN" rm purge_me.txt
"$BIN" purge --all
echo "[+] Purge completed successfully."

# TEST 6.1: Targeted purge by ID
echo -e "\n[TEST 6.1] Targeted purge by ID..."
echo "file to purge by id" > purge_tgt.txt
echo "file to keep in vault" > purge_kp.txt
"$BIN" rm purge_tgt.txt purge_kp.txt

TARGET_ID=$("$BIN" ls --ids | grep "purge_tgt.txt" | awk '{print $1}')
KEEP_ID=$("$BIN" ls --ids | grep "purge_kp.txt" | awk '{print $1}')

"$BIN" purge "$TARGET_ID"

if "$BIN" ls | grep -q "purge_tgt.txt"; then
    echo "[!] Error: Entry $TARGET_ID still visible in active vault!"
    exit 1
fi
if ! "$BIN" ls -a | grep "purge_tgt.txt" | grep -q "PURGED"; then
    echo "[!] Error: Entry $TARGET_ID not marked as PURGED in history!"
    exit 1
fi
if ! "$BIN" ls | grep -q "purge_kp.txt"; then
    echo "[!] Error: Entry $KEEP_ID (purge_kp.txt) was mistakenly purged!"
    exit 1
fi
echo "[+] Specific entry successfully purged by ID without affecting others."

# TEST 6.2: Targeted purge by filename & mutual exclusion check
echo -e "\n[TEST 6.2] Targeted purge by filename & argument conflicts..."
"$BIN" purge purge_kp.txt
if "$BIN" ls | grep -q "purge_kp.txt"; then
    echo "[!] Error: purge_kp.txt still visible in active vault!"
    exit 1
fi
if ! "$BIN" ls -a | grep "purge_kp.txt" | grep -q "PURGED"; then
    echo "[!] Error: purge_kp.txt not marked as PURGED in history!"
    exit 1
fi

# Ensure targets cannot be combined with --all
set +e
output=$("$BIN" purge 1 --all 2>&1)
exit_code=$?
set -e
if [ "$exit_code" -ne 0 ] && echo "$output" | grep -qi "cannot be used with"; then
    echo "[+] Conflict between TARGETS and --all verified."
else
    echo "[!] Error: Expected conflict error when combining targets with --all!"
    exit 1
fi

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
echo -e "\n[TEST 7.1] Testing --no-vault, --no-storage, and -p / --permanent flags..."
echo "permanent junk 1" > perm1.txt
"$BIN" rm --no-vault perm1.txt
if [ -f perm1.txt ]; then
    echo "[!] Error: perm1.txt still exists after --no-vault!"
    exit 1
fi
# Ensure it was not added to the active storage index
if "$BIN" ls | grep -q "perm1.txt"; then
    echo "[!] Error: perm1.txt was indexed in active storage despite --no-vault!"
    exit 1
fi
# Ensure it is recorded in history with PURGED status
if ! "$BIN" ls -a | grep "perm1.txt" | grep -q "PURGED"; then
    echo "[!] Error: perm1.txt not found in history with status PURGED!"
    exit 1
fi

echo "permanent junk 2" > perm2.txt
"$BIN" rm -p perm2.txt
if [ -f perm2.txt ]; then
    echo "[!] Error: perm2.txt still exists after -p!"
    exit 1
fi
if "$BIN" ls | grep -q "perm2.txt"; then
    echo "[!] Error: perm2.txt was indexed in active storage despite -p!"
    exit 1
fi
if ! "$BIN" ls -a | grep "perm2.txt" | grep -q "PURGED"; then
    echo "[!] Error: perm2.txt not found in history with status PURGED!"
    exit 1
fi

echo "permanent junk 3" > perm3.txt
"$BIN" rm --no-storage perm3.txt
if [ -f perm3.txt ]; then
    echo "[!] Error: perm3.txt still exists after --no-storage!"
    exit 1
fi
if "$BIN" ls | grep -q "perm3.txt"; then
    echo "[!] Error: perm3.txt was indexed in storage despite --no-storage!"
    exit 1
fi
if ! "$BIN" ls -a | grep "perm3.txt" | grep -q "PURGED"; then
    echo "[!] Error: perm3.txt not found in history with status PURGED!"
    exit 1
fi

# Ensure inspecting a permanently removed entry displays PURGED and permanently unlinked
PERM1_ID=$("$BIN" ls -a | grep "perm1.txt" | awk '{print $2}')
if ! "$BIN" inspect "$PERM1_ID" | grep -q "PURGED"; then
    echo "[!] Error: inspect output does not show PURGED status for $PERM1_ID!"
    exit 1
fi
if ! "$BIN" inspect "$PERM1_ID" | grep -q "(none - permanently unlinked)"; then
    echo "[!] Error: inspect output does not show (none - permanently unlinked) storage location!"
    exit 1
fi

# Ensure trying to restore a permanently purged file returns an error
if "$BIN" restore "$PERM1_ID" 2>/dev/null; then
    echo "[!] Error: Restoring a permanently purged file should have failed!"
    exit 1
fi

echo "[+] Direct unlinking (--no-storage, --no-vault, -p) unlinks from disk and records in history as PURGED."

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

# TEST 10: Legacy migration test (~/.local/share/recent-inode -> ~/.local/share/rinode)
echo -e "\n[TEST 10] Testing automatic migration of legacy storage and database..."
LEGACY_ROOT="$TEST_ROOT/legacy_test"
mkdir -p "$LEGACY_ROOT"
MIGRATE_DATA="$LEGACY_ROOT/data"
mkdir -p "$MIGRATE_DATA/recent-inode/vault"
echo "legacy file content" > "$MIGRATE_DATA/recent-inode/vault/legacy_test.txt"

sqlite3 "$MIGRATE_DATA/recent-inode/rinode.db" <<EOF
CREATE TABLE entries (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    filename TEXT NOT NULL,
    original_path TEXT NOT NULL,
    vault_path TEXT NOT NULL,
    inode_no INTEGER NOT NULL,
    dev_major INTEGER NOT NULL,
    dev_minor INTEGER NOT NULL,
    mnt_id INTEGER NOT NULL,
    file_size INTEGER NOT NULL,
    mode INTEGER NOT NULL,
    uid INTEGER NOT NULL,
    gid INTEGER NOT NULL,
    is_directory BOOLEAN NOT NULL,
    link_type TEXT NOT NULL,
    symlink_target TEXT,
    quick_fingerprint TEXT,
    status TEXT NOT NULL DEFAULT 'PRESERVED',
    deleted_at TEXT NOT NULL,
    restored_at TEXT
);
INSERT INTO entries VALUES (
    1,
    'legacy_test.txt',
    '$LEGACY_ROOT/legacy_restored.txt',
    '$MIGRATE_DATA/recent-inode/vault/legacy_test.txt',
    12345,
    0, 0, 0,
    19,
    33188,
    1000, 1000,
    0,
    'RENAME_MOVE',
    NULL,
    'abcd',
    'PRESERVED',
    '2026-09-01T00:00:00Z',
    NULL
);
EOF

# Run rinode with XDG_DATA_HOME pointing to MIGRATE_DATA
XDG_DATA_HOME="$MIGRATE_DATA" "$BIN" ls

# Verify directory migration
if [ -d "$MIGRATE_DATA/recent-inode" ]; then
    echo "[!] Error: Legacy directory $MIGRATE_DATA/recent-inode still exists!"
    exit 1
fi
if [ ! -d "$MIGRATE_DATA/rinode/storage" ]; then
    echo "[!] Error: Modern storage directory $MIGRATE_DATA/rinode/storage was not created!"
    exit 1
fi
if [ ! -f "$MIGRATE_DATA/rinode/storage/legacy_test.txt" ]; then
    echo "[!] Error: Legacy file not moved to modern storage location!"
    exit 1
fi

# Verify restoring the migrated entry
XDG_DATA_HOME="$MIGRATE_DATA" "$BIN" restore 1
if [ ! -f "$LEGACY_ROOT/legacy_restored.txt" ]; then
    echo "[!] Error: Migrated legacy file failed to restore!"
    exit 1
fi
MIG_CONTENT=$(cat "$LEGACY_ROOT/legacy_restored.txt")
if [ "$MIG_CONTENT" != "legacy file content" ]; then
    echo "[!] Error: Restored legacy content mismatch: $MIG_CONTENT"
    exit 1
fi
echo "[+] Legacy migration completed and restored successfully."

# Cleanup test directory
rm -rf "$TEST_ROOT"

echo -e "\n=== ALL RINODE INTEGRATION TESTS PASSED SUCCESSFULLY ===\n"

