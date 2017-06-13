#!/bin/zsh
# Copied from fzf
#
# - $SKIM_TMUX               (default: 1)
# - $SKIM_TMUX_HEIGHT        (default: '40%')
# - $SKIM_COMPLETION_TRIGGER (default: '**')
# - $SKIM_COMPLETION_OPTS    (default: empty)

# To use custom commands instead of find, override _skim_compgen_{path,dir}
if ! declare -f _skim_compgen_path > /dev/null; then
  _skim_compgen_path() {
    echo "$1"
    \find -L "$1" \
      -name .git -prune -o -name .svn -prune -o \( -type d -o -type f -o -type l \) \
      -a -not -path "$1" -print 2> /dev/null | sed 's@^\./@@'
  }
fi

if ! declare -f _skim_compgen_dir > /dev/null; then
  _skim_compgen_dir() {
    \find -L "$1" \
      -name .git -prune -o -name .svn -prune -o -type d \
      -a -not -path "$1" -print 2> /dev/null | sed 's@^\./@@'
  }
fi

###########################################################

__skim_generic_path_completion() {
  local base lbuf compgen skim_opts suffix tail skim dir leftover matches nnm
  # (Q) flag removes a quoting level: "foo\ bar" => "foo bar"
  base=${(Q)1}
  lbuf=$2
  compgen=$3
  skim_opts=$4
  suffix=$5
  tail=$6
  [ ${SKIM_TMUX:-1} -eq 1 ] && skim="sk-tmux -d ${SKIM_TMUX_HEIGHT:-40%}" || skim="skim"

  if ! setopt | \grep nonomatch > /dev/null; then
    nnm=1
    setopt nonomatch
  fi
  dir="$base"
  while [ 1 ]; do
    if [ -z "$dir" -o -d ${~dir} ]; then
      leftover=${base/#"$dir"}
      leftover=${leftover/#\/}
      [ -z "$dir" ] && dir='.'
      [ "$dir" != "/" ] && dir="${dir/%\//}"
      dir=${~dir}
      matches=$(eval "$compgen $(printf %q "$dir")" | ${=skim} ${=SKIM_COMPLETION_OPTS} ${=skim_opts} -q "$leftover" | while read item; do
        printf "%q$suffix " "$item"
      done)
      matches=${matches% }
      if [ -n "$matches" ]; then
        LBUFFER="$lbuf$matches$tail"
      fi
      zle redisplay
      break
    fi
    dir=$(dirname "$dir")
    dir=${dir%/}/
  done
  [ -n "$nnm" ] && unsetopt nonomatch
}

_skim_path_completion() {
  __skim_generic_path_completion "$1" "$2" _skim_compgen_path \
    "-m" "" " "
}

_skim_dir_completion() {
  __skim_generic_path_completion "$1" "$2" _skim_compgen_dir \
    "" "/" ""
}

_skim_feed_fifo() (
  rm -f "$1"
  mkfifo "$1"
  cat <&0 > "$1" &
)

_skim_complete() {
  local fifo skim_opts lbuf skim matches post
  fifo="${TMPDIR:-/tmp}/skim-complete-fifo-$$"
  skim_opts=$1
  lbuf=$2
  post="${funcstack[2]}_post"
  type $post > /dev/null 2>&1 || post=cat

  [ ${SKIM_TMUX:-1} -eq 1 ] && skim="sk-tmux -d ${SKIM_TMUX_HEIGHT:-40%}" || skim="skim"

  _skim_feed_fifo "$fifo"
  matches=$(cat "$fifo" | ${=skim} ${=SKIM_COMPLETION_OPTS} ${=skim_opts} -q "${(Q)prefix}" | $post | tr '\n' ' ')
  if [ -n "$matches" ]; then
    LBUFFER="$lbuf$matches"
  fi
  zle redisplay
  rm -f "$fifo"
}

_skim_complete_telnet() {
  _skim_complete '+m' "$@" < <(
    \grep -v '^\s*\(#\|$\)' /etc/hosts | \grep -Fv '0.0.0.0' |
        awk '{if (length($2) > 0) {print $2}}' | sort -u
  )
}

_skim_complete_ssh() {
  _skim_complete '+m' "$@" < <(
    cat <(cat ~/.ssh/config /etc/ssh/ssh_config 2> /dev/null | \grep -i '^host' | \grep -v '*') \
        <(\grep -v '^\s*\(#\|$\)' /etc/hosts | \grep -Fv '0.0.0.0') |
        awk '{if (length($2) > 0) {print $2}}' | sort -u
  )
}

_skim_complete_export() {
  _skim_complete '-m' "$@" < <(
    declare -xp | sed 's/=.*//' | sed 's/.* //'
  )
}

_skim_complete_unset() {
  _skim_complete '-m' "$@" < <(
    declare -xp | sed 's/=.*//' | sed 's/.* //'
  )
}

_skim_complete_unalias() {
  _skim_complete '+m' "$@" < <(
    alias | sed 's/=.*//'
  )
}

skim-completion() {
  local tokens cmd prefix trigger tail skim matches lbuf d_cmds
  setopt localoptions noshwordsplit

  # http://zsh.sourceforge.net/FAQ/zshfaq03.html
  # http://zsh.sourceforge.net/Doc/Release/Expansion.html#Parameter-Expansion-Flags
  tokens=(${(z)LBUFFER})
  if [ ${#tokens} -lt 1 ]; then
    eval "zle ${skim_default_completion:-expand-or-complete}"
    return
  fi

  cmd=${tokens[1]}

  # Explicitly allow for empty trigger.
  trigger=${SKIM_COMPLETION_TRIGGER-'**'}
  [ -z "$trigger" -a ${LBUFFER[-1]} = ' ' ] && tokens+=("")

  tail=${LBUFFER:$(( ${#LBUFFER} - ${#trigger} ))}
  # Kill completion (do not require trigger sequence)
  if [ $cmd = kill -a ${LBUFFER[-1]} = ' ' ]; then
    [ ${SKIM_TMUX:-1} -eq 1 ] && skim="sk-tmux -d ${SKIM_TMUX_HEIGHT:-40%}" || skim="skim"
    matches=$(ps -ef | sed 1d | ${=skim} ${=SKIM_COMPLETION_OPTS} -m | awk '{print $2}' | tr '\n' ' ')
    if [ -n "$matches" ]; then
      LBUFFER="$LBUFFER$matches"
    fi
    zle redisplay
  # Trigger sequence given
  elif [ ${#tokens} -gt 1 -a "$tail" = "$trigger" ]; then
    d_cmds=(${=SKIM_COMPLETION_DIR_COMMANDS:-cd pushd rmdir})

    [ -z "$trigger"      ] && prefix=${tokens[-1]} || prefix=${tokens[-1]:0:-${#trigger}}
    [ -z "${tokens[-1]}" ] && lbuf=$LBUFFER        || lbuf=${LBUFFER:0:-${#tokens[-1]}}

    if eval "type _skim_complete_${cmd} > /dev/null"; then
      eval "prefix=\"$prefix\" _skim_complete_${cmd} \"$lbuf\""
    elif [ ${d_cmds[(i)$cmd]} -le ${#d_cmds} ]; then
      _skim_dir_completion "$prefix" "$lbuf"
    else
      _skim_path_completion "$prefix" "$lbuf"
    fi
  # Fall back to default completion
  else
    eval "zle ${skim_default_completion:-expand-or-complete}"
  fi
}

[ -z "$skim_default_completion" ] &&
  skim_default_completion=$(bindkey '^I' | \grep -v undefined-key | awk '{print $2}')

zle     -N   skim-completion
bindkey '^I' skim-completion
