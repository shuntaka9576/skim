#!/bin/bash

# Copied some part of fzf's key-bindings.bash

if [[ $- =~ i ]]; then

__skcmd() {
  [ "${SK_TMUX:-1}" != 0 ] && echo "sk-tmux -d${SK_TMUX_HEIGHT:-40%}" || echo "skim"
}

__skim_history__() (
  local line
  shopt -u nocaseglob nocasematch
  line=$(
    HISTTIMEFORMAT= history |
    eval "$(__skcmd) --tiebreak=score,-index $SK_CTRL_R_OPTS" |
    command grep '^ *[0-9]') &&
    if [[ $- =~ H ]]; then
      sed 's/^ *\([0-9]*\)\** .*/!\1/' <<< "$line"
    else
      sed 's/^ *\([0-9]*\)\** *//' <<< "$line"
    fi
)

if [[ ! -o vi ]]; then
  # Required to refresh the prompt after skim
  bind '"\er": redraw-current-line'
  bind '"\e^": history-expand-line'

  # CTRL-R - Paste the selected command from history into the command line
  bind '"\C-r": " \C-e\C-u`__skim_history__`\e\C-e\e^\er"'

else
  # We'd usually use "\e" to enter vi-movement-mode so we can do our magic,
  # but this incurs a very noticeable delay of a half second or so,
  # because many other commands start with "\e".
  # Instead, we bind an unused key, "\C-x\C-a",
  # to also enter vi-movement-mode,
  # and then use that thereafter.
  # (We imagine that "\C-x\C-a" is relatively unlikely to be in use.)
  bind '"\C-x\C-a": vi-movement-mode'

  bind '"\C-x\C-e": shell-expand-line'
  bind '"\C-x\C-r": redraw-current-line'
  bind '"\C-x^": history-expand-line'

  # CTRL-R - Paste the selected command from history into the command line
  bind '"\C-r": "\C-x\C-addi$(__skim_history__)\C-x\C-e\C-x^\C-x\C-a$a\C-x\C-r"'
  bind -m vi-command '"\C-r": "i\C-r"'

fi

fi
