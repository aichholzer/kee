#!/usr/bin/env bash

KEE=.kees
KEE_DIR=${HOME}/${KEE}
[ -d "${KEE_DIR}" ] && echo " 💥 You seem to have 'kee' installed already." && exit

mkdir ${KEE_DIR} && cd $_
git clone https://github.com/aichholzer/kee.git . --depth=1 2>/dev/null & PID=$!
printf "\n Downloading"
while kill -0 $PID 2> /dev/null; do
  printf "."
  sleep 0.07
done
echo -ne "\r\033[0K"

IFS=$'\n'
for line in `cat ./kee.art`
do echo ${line}; sleep 0.02; done

CONFIG=$(cat << EOF
   ### Kee
   export KEE_DIR="\$HOME/${KEE}"
     [ -s "\$KEE_DIR/kee.sh" ] && \. "\$KEE_DIR/kee.sh"
     [ -s "\$KEE_DIR/completions" ] && \. "\$KEE_DIR/completions"
EOF)

echo " ✔ Append the following lines to your profile (eg: ~/.bashrc, ~/.bash_profile, ~/.zshrc, ~/.profile) file:"
echo "   (For your convenience; I have copied these lines to your paste board.)"
echo
echo "${CONFIG}"
echo "${CONFIG}" | pbcopy
echo
echo " ✔ Restart your terminal & enjoy."
