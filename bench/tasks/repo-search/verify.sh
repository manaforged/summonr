test -f .bench-answer \
  && diff <(grep -rl "apply_patch" src/tools/workspace/ | xargs -n1 basename | sort) <(sort .bench-answer)
