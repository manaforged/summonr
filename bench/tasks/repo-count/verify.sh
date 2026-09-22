test -f .bench-answer && [ "$(cat .bench-answer | tr -d '[:space:]')" = "$(find src/tools -maxdepth 1 -name '*.rs' | wc -l | tr -d ' ')" ]
