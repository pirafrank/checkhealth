# Same as ../rc, after a 0.3 s start-up delay (a heavy rc file).
sleep 0.3
alias bench-alias-1='ls'
alias bench-alias-2='ls'
alias bench-alias-3='ls'
alias bench-alias-4='ls'
bench-func-1() { :; }
bench-func-2() { :; }
bench-func-3() { :; }
bench-func-4() { :; }
