# Key bindings
# ------------
if [[ $- == *i* ]]; then

# CTRL-T - Paste the selected file path(s) into the command line
__fsel() {
  local cmd="${SKIM_CTRL_T_COMMAND:-"command find -L . \\( -path '*/\\.*' -o -fstype 'dev' -o -fstype 'proc' \\) -prune \
    -o -type f -print \
    -o -type d -print \
    -o -type l -print 2> /dev/null | sed 1d | cut -b3-"}"
  eval "$cmd" | $(__skimcmd) -m | while read item; do
    printf '%q ' "$item"
  done
  echo
}

__skimcmd() {
  [ ${SKIM_TMUX:-1} -eq 1 ] && echo "sk-tmux -d${SKIM_TMUX_HEIGHT:-40%}" || echo "skim"
}

skim-file-widget() {
  LBUFFER="${LBUFFER}$(__fsel)"
  zle redisplay
}
zle     -N   skim-file-widget
bindkey '^T' skim-file-widget

# ALT-C - cd into the selected directory
skim-cd-widget() {
  local cmd="${SKIM_ALT_C_COMMAND:-"command find -L . \\( -path '*/\\.*' -o -fstype 'dev' -o -fstype 'proc' \\) -prune \
    -o -type d -print 2> /dev/null | sed 1d | cut -b3-"}"
  cd "${$(eval "$cmd" | $(__skimcmd) +m):-.}"
  zle reset-prompt
}
zle     -N    skim-cd-widget
bindkey '\ec' skim-cd-widget

# CTRL-R - Paste the selected command from history into the command line
skim-history-widget() {
  local selected num
  selected=( $(fc -l 1 | $(__skimcmd) -m -n1..,.. --tiebreak=-index -q "${LBUFFER//$/\\$}") )
  if [ -n "$selected" ]; then
    num=$selected[1]
    if [ -n "$num" ]; then
      zle vi-fetch-history -n $num
    fi
  fi
  zle redisplay
}
zle     -N   skim-history-widget
bindkey '^R' skim-history-widget

fi

