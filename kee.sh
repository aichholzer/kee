function __mask () {
  local n=5                        # Take this many characters
  local a="${1:0:${#1}-n}"         # Take all characters, except the last "n"
  local b="${1:${#1}-n}"           # Take the the last "n" characters

  local VALUE=$(printf "%s%s\n" "${a//?/*}" "${b}") # Replace "char" with *
  ! [[ ${2} =~ '^[0-9]+$' ]] && echo ${VALUE} || echo ${VALUE} | tail -c ${2}
}

function __storeAccounts () {
  security add-generic-password -C note -a kee.sh -s kee.sh -U -w ${1}
}

function __loadAccounts () {
  local DEFAULT_RESPONSE=${1}
  local ACCOUNTS=$(security find-generic-password -a kee.sh -s kee.sh -w 2>/dev/null)
  [ ! "${ACCOUNTS}" ] && [ "${1}" ] && echo ${1} || echo ${ACCOUNTS}
}

function __loadAccount () {
  local PROFILE=${1}
  ACCOUNTS=$(__loadAccounts)
  [ ! "${ACCOUNTS}" ] && echo "" && return

  local ACCOUNT=$(echo ${ACCOUNTS} | jq -r '.[] | select(.profile=="'${PROFILE}'")')
  echo ${ACCOUNT}
}

function __getProp () {
  local VALUE=$(echo ${1} | jq -r '.properties.'${2}' | select (. != null)')
  [ "${3}" = "mask" ] && echo $(__mask ${VALUE} ${4}) || echo ${VALUE}
}

function kee () {
  local COMMAND=${1}
  local PROFILE=${2}
  local ACCOUNT
  local ACCOUNTS
  local RUN
  local TEMP=false
  local TF_AUTO_APPROVE=""

  ## Parse special CLI arguments.
  while [ $# -gt 0 ]; do
    case "${1}" in
      -r|--run)
        RUN="${2}"
        ;;
      -t|--temp)
        TEMP=true
        ;;
      --approve)
        TF_AUTO_APPROVE="-auto-approve"
        ;;
      *)
    esac
    shift
  done

  if [ "${COMMAND}" = "ls" ]; then
    echo ""
    ACCOUNTS=$(echo $(__loadAccounts) | jq -r '.[] | .profile' | sed "s/\w*/ • /")
    [ ! "${ACCOUNTS}" ] && echo " 💥 No accounts have been found.\n    Get started with: kee add ..." || echo ${ACCOUNTS}
  elif [ "${COMMAND}" = "show" ]; then
    if [ ! "${PROFILE}" ] && [ ! "${AWS_PROFILE}" ]; then
      echo "\n 💥 No profile is currently selected."
    else
      [ ! "${PROFILE}" ] && PROFILE=${AWS_PROFILE}
      ACCOUNT=$(__loadAccount ${PROFILE})
      [ ! "${ACCOUNT}" ] && echo "\n 💥 This profile does not exist." && return

      echo
      echo " • Profile: \t\t${PROFILE}"
      echo " • Account ID: \t\t$(__getProp ${ACCOUNT} account)"
      echo " • Region: \t\t$(__getProp ${ACCOUNT} region)"
      echo " • Access key: \t\t$(__getProp ${ACCOUNT} access_key mask 15)"
      echo " • Secret access key: \t$(__getProp ${ACCOUNT} secret_access_key mask 15)"
      echo " • Environment: \t$(__getProp ${ACCOUNT} environment)"
      local DOMAIN=$(__getProp ${ACCOUNT} domain)
      [ "${DOMAIN}" ] && echo " • Domain: \t\t${DOMAIN}"
    fi

    return
  elif [ "${COMMAND}" = "add" ]; then
    [ ! "${PROFILE}" ] && echo "\n 💥 You need to specify an account name." && return

    echo "\n Profile name: ${PROFILE}"
    read "ACCOUNT_ID? Account ID: "
    read "ACCESS_KEY? Access key: "
    read "SECRET_ACCESS_KEY? Secret access key: "
    read "DOMAIN? Domain: "
    read "REGION? Region (Default: "ap-southeast-2"): "
    read "OUTPUT? Output (Default: "json"): "
    read "ENVIRONMENT? Environment (Default: "dev"): "

    [ ! "${REGION}" ] && REGION=ap-southeast-2
    [ ! "${OUTPUT}" ] && OUTPUT=json
    [ ! "${ENVIRONMENT}" ] && ENVIRONMENT=dev

    ACCOUNTS=$(echo $(__loadAccounts '[]') | jq -c '. + [{
      "profile": "'${PROFILE}'",
      "properties": {
        "account": "'${ACCOUNT_ID}'",
        "access_key": "'${ACCESS_KEY}'",
        "secret_access_key": "'${SECRET_ACCESS_KEY}'",
        "region": "'${REGION}'",
        "output": "'${OUTPUT}'",
        "domain": "'${DOMAIN}'",
        "environment": "'${ENVIRONMENT}'"
      }
    }]')

    __storeAccounts ${ACCOUNTS}

    ## Write the account to the AWS CLI config files.
    echo "\n[profile ${PROFILE}]\nregion = ${REGION}\noutput = ${OUTPUT}" >> ~/.aws/config

    return
  elif [ "${COMMAND}" = "remove" ]; then
    ACCOUNT=$(__loadAccount ${PROFILE})
    [ ! "${ACCOUNT}" ] && echo "\n 💥 This profile does not exist, nothing to remove..." && return

    echo "\n ⚠️  This will permanently remove \"${PROFILE}\""
    read "CONTINUE? Type \"yes\" to continue: "

    if [ "${CONTINUE}" = "yes" ]; then
      ACCOUNTS=$(echo $(__loadAccounts) | jq -c 'del(.[] | select(.profile == "'${PROFILE}'"))')
      __storeAccounts ${ACCOUNTS}
      echo "\n \"${PROFILE}\" has been removed."

      # If the profile being removed is the currently selected profile, then clear it.
      if [ "${PROFILE}" = "${AWS_PROFILE}" ]; then
        unset AWS_PROFILE AWS_DEFAULT_PROFILE
        unset AWS_ACCESS_KEY_ID AWS_SECRET_ACCESS_KEY
      fi

      ## Remove the account to the AWS CLI config files.
      sed -i '' '/\[profile '${PROFILE}'\]/,/^$/d' ~/.aws/*
      sed -i '' 'N;/^\n$/d;P;D' ~/.aws/*

      kee ls
    fi
  elif [ "${COMMAND}" = "use" ]; then
    ACCOUNT=$(__loadAccount ${PROFILE})
    [ ! "${ACCOUNT}" ] && echo "\n 💥 This profile does not exist." && return

    ## If a "run" command was specified:
    ## - Get the accounts credentials,
    ## - obtain a short-lived set of credentials through AWS STS,
    ## - expose those to the sub-process only,
    ## - run the command
    if [ "${RUN}" ]; then
      local AK=$(__getProp ${ACCOUNT} access_key)
      local SK=$(__getProp ${ACCOUNT} secret_access_key)
      RUN=($(echo ${RUN} | tr " " "\n"))

      ## Generate a temporary set of credentials to perform this action
      if [ "${TEMP}" = true ]; then
        local TC=$(AWS_ACCESS_KEY_ID=${AK} AWS_SECRET_ACCESS_KEY=${SK} aws sts get-session-token --duration-seconds 900 --output json | jq -r '.Credentials')
        TC=(${$(echo ${TC} | jq -r '.AccessKeyId, .SecretAccessKey, .SessionToken')//$'\n'/ })
        TC=($(echo ${TC} | tr " " "\n"))
        (AWS_ACCESS_KEY_ID=${TC[1]} AWS_SECRET_ACCESS_KEY=${TC[2]} AWS_SESSION_TOKEN=${TC[3]} "${RUN[@]}")
      else
        (AWS_ACCESS_KEY_ID=${AK} AWS_SECRET_ACCESS_KEY=${SK} "${RUN[@]}")
      fi
    else
      export AWS_PROFILE=${PROFILE}
      export AWS_DEFAULT_PROFILE=${PROFILE}
      export AWS_ACCOUNT_ID=$(__getProp ${ACCOUNT} account)
      export AWS_REGION=$(__getProp ${ACCOUNT} region)
      export AWS_ACCESS_KEY_ID=$(__getProp ${ACCOUNT} access_key)
      export AWS_SECRET_ACCESS_KEY=$(__getProp ${ACCOUNT} secret_access_key)
      export DOMAIN=$(__getProp ${ACCOUNT} domain)
      export ENVIRONMENT=$(__getProp ${ACCOUNT} environment)
      export TERRAFORM_BUCKET=$(__getProp ${ACCOUNT} terraform_bucket)

      echo "\n ✔ Now using profile \"${PROFILE}\""

      ## If we are in a Terraform directory, automatically initialize with the current environment.
      if [ -f "./main.tf" ]; then
          echo "   Initializing the current Terraform environment: \"${ENVIRONMENT}\""
          kee tf
      fi
    fi
  elif [ "${COMMAND}" = "export" ]; then
    local SAFETY_NOTICE="\n ⚠️  You are about to export PLACEHOLDER. Make sure you keep this output safe & secure."
    if [ ! "${PROFILE}" ]; then
      SAFETY_NOTICE=$(echo ${SAFETY_NOTICE} | sed -r 's/[PLACEHOLDER]+/all accounts/g')
    else
      SAFETY_NOTICE=$(echo ${SAFETY_NOTICE} | sed -r 's/[PLACEHOLDER]+/the '\""${PROFILE}"\"' account/g')
    fi

    echo ${SAFETY_NOTICE}
    read "EXPORT_FILE?    Enter a file name (Default: "kee.json"): "
    [ ! "${EXPORT_FILE}" ] && EXPORT_FILE="kee.json"
    [ ! "${PROFILE}" ] && __loadAccounts '[]' | jq -r '.' > ${EXPORT_FILE} || __loadAccount ${PROFILE} > ${EXPORT_FILE}
  elif [ "${COMMAND}" = "tf" ]; then
    [ ! "${ENVIRONMENT}" ] && echo "\n 💥 The ENVIRONMENT must be set before running this action." && return

    ACTION=${PROFILE}
    [ ! "${ACTION}" ] && ACTION=init

    ACTIONS=("validate plan apply destroy refresh console")
    if [[ " ${ACTIONS[*]} " =~ " ${ACTION} " ]]; then
      terraform ${ACTION} ${TF_AUTO_APPROVE} -var-file=${ENVIRONMENT}.tfvars
    elif [ "${ACTION}" = "init" ]; then
      echo
      READ_BUCKET_NAME=false
      if [ -z "${TERRAFORM_BUCKET}" ]; then
        read "TERRAFORM_BUCKET? State bucket (S3): "
        READ_BUCKET_NAME=true
      fi

      if [ "${TERRAFORM_BUCKET}" ]; then
        terraform init -reconfigure -backend-config="bucket=${TERRAFORM_BUCKET}" | grep -E 'successfully' | sed "s/\w*/ ✔ /"
        if [ "${READ_BUCKET_NAME}" = true ]; then
          echo
          read "SAVE_BUCKET_NAME? Did you want to save this bucket to the current profile? "

          if [ "${SAVE_BUCKET_NAME}" = "yes" ]; then
            ACCOUNTS=$(echo $(__loadAccounts))
            ACCOUNTS=$(echo ${ACCOUNTS} | jq -c 'map((select(.profile=="'${AWS_PROFILE}'") | .properties.terraform_bucket) |= "'${TERRAFORM_BUCKET}'")')
            __storeAccounts ${ACCOUNTS}
          fi
        fi
      else
        terraform init
      fi
    fi
  else
    echo "\n 💥 You need to give me a command."

    echo "\n Currently active profile:"
    kee show

    echo "\n Available profiles:"
    kee ls

    return
  fi
}
