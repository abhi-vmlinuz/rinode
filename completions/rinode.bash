# bash completion for rinode

_rinode() {
    local cur prev words cword

    if declare -F _init_completion >/dev/null 2>&1; then
        _init_completion -n = || return
    else
        cur="${COMP_WORDS[COMP_CWORD]}"
        prev="${COMP_WORDS[COMP_CWORD-1]}"
        words=("${COMP_WORDS[@]}")
        cword=$COMP_CWORD
    fi

    local commands="rm ls restore inspect purge tui init exclude help"

    if [[ $cword -eq 1 ]]; then
        COMPREPLY=( $(compgen -W "${commands}" -- "$cur") )
        return 0
    fi

    local subcmd="${words[1]}"

    case "$subcmd" in
        rm)
            if [[ "$cur" == -* ]]; then
                COMPREPLY=( $(compgen -W "--no-vault --permanent -p --force -f -r -R --recursive -v --verbose -i --interactive -d --dir --help -h" -- "$cur") )
            else
                if declare -F _filedir >/dev/null 2>&1; then
                    _filedir
                else
                    COMPREPLY=( $(compgen -f -- "$cur") )
                fi
            fi
            ;;
        ls)
            COMPREPLY=( $(compgen -W "--limit -n --all -a --help -h" -- "$cur") )
            ;;
        restore)
            if [[ "$cur" == -* ]]; then
                COMPREPLY=( $(compgen -W "--keep-vault -k --force -f --help -h" -- "$cur") )
            else
                local active_entries
                active_entries=$(rinode ls --ids 2>/dev/null | tr '\t' ' ')
                COMPREPLY=( $(compgen -W "${active_entries}" -- "$cur") )
            fi
            ;;
        inspect)
            if [[ "$cur" == -* ]]; then
                COMPREPLY=( $(compgen -W "--help -h" -- "$cur") )
            else
                local active_ids
                active_ids=$(rinode ls --ids 2>/dev/null | awk '{print $1}')
                COMPREPLY=( $(compgen -W "${active_ids}" -- "$cur") )
            fi
            ;;
        purge)
            COMPREPLY=( $(compgen -W "--days -d --all --help -h" -- "$cur") )
            ;;
        tui)
            COMPREPLY=( $(compgen -W "--help -h" -- "$cur") )
            ;;
        init)
            if [[ "$cur" == -* ]]; then
                COMPREPLY=( $(compgen -W "--alias --alias-rm --help -h" -- "$cur") )
            else
                COMPREPLY=( $(compgen -W "fish bash zsh" -- "$cur") )
            fi
            ;;
        exclude)
            if [[ "$prev" == "--test" || "$prev" == "-t" ]]; then
                if declare -F _filedir >/dev/null 2>&1; then
                    _filedir
                else
                    COMPREPLY=( $(compgen -f -- "$cur") )
                fi
            elif [[ "$cur" == -* ]]; then
                COMPREPLY=( $(compgen -W "--list -l --test -t --remove -r --help -h" -- "$cur") )
            else
                if declare -F _filedir >/dev/null 2>&1; then
                    _filedir
                else
                    COMPREPLY=( $(compgen -f -- "$cur") )
                fi
            fi
            ;;
        *)
            ;;
    esac
}

complete -F _rinode rinode
complete -F _rinode r 2>/dev/null || true

