# fish completion for rinode

# Disable file completion by default
complete -c rinode -f

# Main subcommands
complete -c rinode -n "__fish_use_subcommand" -a "rm" -d "Safely delete files or directories by moving them into the vault"
complete -c rinode -n "__fish_use_subcommand" -a "ls" -d "List recently deleted files"
complete -c rinode -n "__fish_use_subcommand" -a "restore" -d "Restore a deleted file or directory"
complete -c rinode -n "__fish_use_subcommand" -a "inspect" -d "Inspect detailed metadata of an entry"
complete -c rinode -n "__fish_use_subcommand" -a "purge" -d "Purge expired files from the vault"
complete -c rinode -n "__fish_use_subcommand" -a "tui" -d "Launch interactive terminal dashboard"
complete -c rinode -n "__fish_use_subcommand" -a "init" -d "Generate shell integration script"
complete -c rinode -n "__fish_use_subcommand" -a "exclude" -d "Manage or test pattern and directory exclusions"
complete -c rinode -n "__fish_use_subcommand" -a "help" -d "Print help information"

# Flags for rm
complete -c rinode -n "__fish_seen_subcommand_from rm" -F
complete -c rinode -n "__fish_seen_subcommand_from rm" -s p -l permanent -d "Permanently delete without vaulting"
complete -c rinode -n "__fish_seen_subcommand_from rm" -s f -l force -d "Force deletion without warnings"
complete -c rinode -n "__fish_seen_subcommand_from rm" -s r -s R -l recursive -d "Remove directories recursively (POSIX compatibility)"
complete -c rinode -n "__fish_seen_subcommand_from rm" -s v -l verbose -d "Explain what is being done"
complete -c rinode -n "__fish_seen_subcommand_from rm" -s i -l interactive -d "Prompt before removal"
complete -c rinode -n "__fish_seen_subcommand_from rm" -s d -l dir -d "Remove empty directories"
complete -c rinode -n "__fish_seen_subcommand_from rm" -s h -l help -d "Print help"

# Flags for ls
complete -c rinode -n "__fish_seen_subcommand_from ls" -s n -l limit -d "Maximum entries to display"
complete -c rinode -n "__fish_seen_subcommand_from ls" -s a -l all -d "Show all entries including restored/purged"
complete -c rinode -n "__fish_seen_subcommand_from ls" -s h -l help -d "Print help"

# Flags & dynamic completion for restore
complete -c rinode -n "__fish_seen_subcommand_from restore" -s k -l keep-vault -d "Snapshot fork mode via CoW/Reflink"
complete -c rinode -n "__fish_seen_subcommand_from restore" -s f -l force -d "Overwrite if destination exists"
complete -c rinode -n "__fish_seen_subcommand_from restore" -s h -l help -d "Print help"
complete -c rinode -n "__fish_seen_subcommand_from restore" -a "(rinode ls --ids 2>/dev/null)"

# Flags & dynamic completion for inspect
complete -c rinode -n "__fish_seen_subcommand_from inspect" -s h -l help -d "Print help"
complete -c rinode -n "__fish_seen_subcommand_from inspect" -a "(rinode ls --ids 2>/dev/null)"

# Flags for purge
complete -c rinode -n "__fish_seen_subcommand_from purge" -s d -l days -d "Purge entries older than N days"
complete -c rinode -n "__fish_seen_subcommand_from purge" -l all -d "Force purge all entries"
complete -c rinode -n "__fish_seen_subcommand_from purge" -s h -l help -d "Print help"

# Flags for init
complete -c rinode -n "__fish_seen_subcommand_from init" -a "fish bash zsh" -d "Supported shell"
complete -c rinode -n "__fish_seen_subcommand_from init" -l alias-rm -d "Also alias rm to rinode rm"
complete -c rinode -n "__fish_seen_subcommand_from init" -s h -l help -d "Print help"

# Flags for exclude
complete -c rinode -n "__fish_seen_subcommand_from exclude" -F
complete -c rinode -n "__fish_seen_subcommand_from exclude" -s l -l list -d "List all active exclusion rules"
complete -c rinode -n "__fish_seen_subcommand_from exclude" -s t -l test -r -F -d "Dry-run test path against exclusion filters"
complete -c rinode -n "__fish_seen_subcommand_from exclude" -s r -l remove -r -d "Remove an existing rule by index or text"
complete -c rinode -n "__fish_seen_subcommand_from exclude" -s h -l help -d "Print help"

# General options
complete -c rinode -s h -l help -d "Print help"
complete -c rinode -s V -l version -d "Print version"

# Wrap 'r' shortcut to rinode completions if defined
complete -c r -w rinode
