# Bash completion for kee
_kee_completion() {
  local cur prev opts
  COMPREPLY=()
  cur="${COMP_WORDS[COMP_CWORD]}"
  prev="${COMP_WORDS[COMP_CWORD-1]}"

  case ${COMP_CWORD} in
    1)
      opts="add use ls current rm set run aws help"
      COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
      return 0
      ;;
    2)
      case "${prev}" in
        use|rm|set|run|aws)
          # Get account names dynamically
          local accounts=$(${COMP_WORDS[0]} ls --names 2>/dev/null)
          COMPREPLY=( $(compgen -W "${accounts}" -- "${cur}") )
          return 0
          ;;
        ls)
          # Complete ls command flags
          opts="--names --help"
          COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
          return 0
          ;;
        *)
          ;;
      esac
      ;;
    *)
      # Handle flags for commands that accept them after the profile name
      case "${COMP_WORDS[1]}" in
        set)
          opts="--production --no-production"
          COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
          return 0
          ;;
      esac
      ;;
  esac
}

complete -F _kee_completion kee
