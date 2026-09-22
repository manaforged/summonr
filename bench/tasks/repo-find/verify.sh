test -f .bench-answer \
  && diff <(sed -n '/pub(crate) struct ToolOutput/,/^}/p' src/tools.rs | grep -oE "pub\(crate\) [a-z_]+:" | sed 's/pub(crate) //;s/://') <(grep -v '^$' .bench-answer)
