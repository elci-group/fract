Output fract-demo.gif

# Larger terminal for demo review
Set Shell "bash"
Set FontSize 18
Set Width 900
Set Height 500
Set TypingSpeed 30ms
Set Theme "Catppuccin Mocha"

Hide
Type "cd /tmp/fract-anim-test && rm -rf fract-anim-test && mkdir fract-anim-test && cd fract-anim-test"
Enter
Sleep 500ms

Show
Type "/home/sal/fract/target/release/fract init --path . --color always"
Enter
Sleep 2.5s

Type "clear"
Enter
Sleep 500ms
Type "/home/sal/fract/target/release/fract index --color always"
Enter
Sleep 3.5s

Screenshot fract-demo-final.png
Sleep 1s