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

pub fn generate_init(shell_opt: Option<&str>, alias_name: &str, alias_rm: bool) -> Result<String, String> {
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

    let alias_trim = alias_name.trim();
    let has_alias = !alias_trim.is_empty()
        && !alias_trim.eq_ignore_ascii_case("none")
        && !alias_trim.eq_ignore_ascii_case("false");

    match shell.to_lowercase().as_str() {
        "fish" => {
            let mut out = String::from(
                "# rinode shell integration for fish\n\
                 # To enable, add this line to ~/.config/fish/config.fish:\n\
                 #   rinode init fish | source\n\n",
            );

            if has_alias {
                out.push_str(&format!(
                    "function {a} --description \"rinode shortcut\"\n\
                         if test (count $argv) -eq 0\n\
                             rinode tui\n\
                         else if test \"$argv[1]\" = \"ls\"\n\
                             rinode ls $argv[2..-1]\n\
                         else if test \"$argv[1]\" = \"restore\"\n\
                             rinode restore $argv[2..-1]\n\
                         else if test \"$argv[1]\" = \"inspect\"\n\
                             rinode inspect $argv[2..-1]\n\
                         else if test \"$argv[1]\" = \"purge\"\n\
                             rinode purge $argv[2..-1]\n\
                         else\n\
                             rinode rm $argv\n\
                         end\n\
                     end\n\n\
                     # Inherit completions from rinode\n\
                     complete -c {a} -w rinode\n",
                    a = alias_trim
                ));
            }

            if alias_rm {
                out.push_str("\nalias rm=\"rinode rm\"\n");
            }

            Ok(out)
        }

        "bash" => {
            let mut out = String::from(
                "# rinode shell integration for bash\n\
                 # To enable, add this line to ~/.bashrc:\n\
                 #   eval \"$(rinode init bash)\"\n\n",
            );

            if has_alias {
                out.push_str(&format!(
                    "{a}() {{\n\
                         if [ $# -eq 0 ]; then\n\
                             rinode tui\n\
                         elif [ \"$1\" = \"ls\" ]; then\n\
                             rinode ls \"${{@:2}}\"\n\
                         elif [ \"$1\" = \"restore\" ]; then\n\
                             rinode restore \"${{@:2}}\"\n\
                         elif [ \"$1\" = \"inspect\" ]; then\n\
                             rinode inspect \"${{@:2}}\"\n\
                         elif [ \"$1\" = \"purge\" ]; then\n\
                             rinode purge \"${{@:2}}\"\n\
                         else\n\
                             rinode rm \"$@\"\n\
                         fi\n\
                     }}\n\n\
                     # Inherit bash completion\n\
                     complete -F _rinode {a} 2>/dev/null || true\n",
                    a = alias_trim
                ));
            }

            if alias_rm {
                out.push_str("\nalias rm=\"rinode rm\"\n");
            }

            Ok(out)
        }

        "zsh" => {
            let mut out = String::from(
                "# rinode shell integration for zsh\n\
                 # To enable, add this line to ~/.zshrc:\n\
                 #   eval \"$(rinode init zsh)\"\n\n",
            );

            if has_alias {
                out.push_str(&format!(
                    "{a}() {{\n\
                         if [ $# -eq 0 ]; then\n\
                             rinode tui\n\
                         elif [ \"$1\" = \"ls\" ]; then\n\
                             rinode ls \"${{@:2}}\"\n\
                         elif [ \"$1\" = \"restore\" ]; then\n\
                             rinode restore \"${{@:2}}\"\n\
                         elif [ \"$1\" = \"inspect\" ]; then\n\
                             rinode inspect \"${{@:2}}\"\n\
                         elif [ \"$1\" = \"purge\" ]; then\n\
                             rinode purge \"${{@:2}}\"\n\
                         else\n\
                             rinode rm \"$@\"\n\
                         fi\n\
                     }}\n\n\
                     # Inherit zsh completion\n\
                     compdef _rinode {a} 2>/dev/null || true\n",
                    a = alias_trim
                ));
            }

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
