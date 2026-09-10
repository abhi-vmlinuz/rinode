use std::path::Path;

pub fn detect_shell() -> Option<String> {
    if let Ok(sh) = std::env::var("SHELL") {
        let p = Path::new(&sh);
        if let Some(name) = p.file_name().and_then(|s| s.to_str()) {
            match name {
                "fish" => return Some("fish".to_string()),
                "bash" => return Some("bash".to_string()),
                "zsh" => return Some("zsh".to_string()),
                _ => {}
            }
        }
    }
    None
}

pub fn generate_init(shell_opt: Option<&str>, alias_rm: bool) -> Result<String, String> {
    let shell = match shell_opt {
        Some(s) => s.to_string(),
        None => match detect_shell() {
            Some(detected) => detected,
            None => {
                return Err(
                    "Could not auto-detect shell from $SHELL environment variable.\n\
                     Please specify your shell: rinode init <fish|bash|zsh>\n\n\
                     Setup instructions:\n\
                       Fish (~/.config/fish/config.fish):  rinode init fish | source\n\
                       Bash (~/.bashrc):                   eval \"$(rinode init bash)\"\n\
                       Zsh (~/.zshrc):                     eval \"$(rinode init zsh)\"".to_string()
                );
            }
        }
    };

    match shell.to_lowercase().as_str() {
        "fish" => {
            let mut out = String::from(
                r#"# rinode shell integration for fish
# To enable, add this line to ~/.config/fish/config.fish:
#   rinode init fish | source

function r --description "rinode smart shortcut"
    if test (count $argv) -eq 0
        rinode tui
    else if test "$argv[1]" = "ls"
        rinode ls $argv[2..-1]
    else if test "$argv[1]" = "restore"
        rinode restore $argv[2..-1]
    else if test "$argv[1]" = "inspect"
        rinode inspect $argv[2..-1]
    else if test "$argv[1]" = "purge"
        rinode purge $argv[2..-1]
    else
        rinode rm $argv
    end
end

# Inherit completions from rinode
complete -c r -w rinode
"#,
            );

            if alias_rm {
                out.push_str("\nalias rm=\"rinode rm\"\n");
            }

            Ok(out)
        }

        "bash" => {
            let mut out = String::from(
                r#"# rinode shell integration for bash
# To enable, add this line to ~/.bashrc:
#   eval "$(rinode init bash)"

r() {
    if [ $# -eq 0 ]; then
        rinode tui
    elif [ "$1" = "ls" ]; then
        rinode ls "${@:2}"
    elif [ "$1" = "restore" ]; then
        rinode restore "${@:2}"
    elif [ "$1" = "inspect" ]; then
        rinode inspect "${@:2}"
    elif [ "$1" = "purge" ]; then
        rinode purge "${@:2}"
    else
        rinode rm "$@"
    fi
}

# Inherit bash completion
complete -F _rinode r 2>/dev/null || true
"#,
            );

            if alias_rm {
                out.push_str("\nalias rm=\"rinode rm\"\n");
            }

            Ok(out)
        }

        "zsh" => {
            let mut out = String::from(
                r#"# rinode shell integration for zsh
# To enable, add this line to ~/.zshrc:
#   eval "$(rinode init zsh)"

r() {
    if [ $# -eq 0 ]; then
        rinode tui
    elif [ "$1" = "ls" ]; then
        rinode ls "${@:2}"
    elif [ "$1" = "restore" ]; then
        rinode restore "${@:2}"
    elif [ "$1" = "inspect" ]; then
        rinode inspect "${@:2}"
    elif [ "$1" = "purge" ]; then
        rinode purge "${@:2}"
    else
        rinode rm "$@"
    fi
}

# Inherit zsh completion
compdef _rinode r 2>/dev/null || true
"#,
            );

            if alias_rm {
                out.push_str("\nalias rm=\"rinode rm\"\n");
            }

            Ok(out)
        }

        _ => Err(format!(
            "Unsupported shell '{}'. Supported shells: fish, bash, zsh\n\n\
             Setup instructions:\n\
               Fish (~/.config/fish/config.fish):  rinode init fish | source\n\
               Bash (~/.bashrc):                   eval \"$(rinode init bash)\"\n\
               Zsh (~/.zshrc):                     eval \"$(rinode init zsh)\"",
            shell
        )),
    }
}
