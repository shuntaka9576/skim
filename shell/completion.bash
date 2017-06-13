_skim_complete_kill() {
    [ -n "${COMP_WORDS[COMP_CWORD]}" ] && return 1

    local selected skim
    [ "${SKIM_TMUX:-1}" != 0 ] && skim="sk-tmux -d ${SKIM_TMUX_HEIGHT:-40%}" || skim="sk"
    tput sc
    selected=$(ps -ef | sed 1d | $skim -m $SKIM_COMPLETION_OPTS | awk '{print $2}' | tr '\n' ' ')
    tput rc

    if [ -n "$selected" ]; then
        COMPREPLY=( "$selected" )
        return 0
    fi
}

complete -F _fzf_complete_kill -o nospace -o default -o bashdefault kill
