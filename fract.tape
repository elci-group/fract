Output fract-refined.gif

# Set up terminal
Set Shell "bash"
Set FontSize 14
Set Width 800
Set Height 450
Set TypingSpeed 50ms
Set Theme "Catppuccin Mocha"

# Hide initial prompt while setting up workspace
Hide
Type "cd /tmp && rm -rf fract-anim-test && mkdir fract-anim-test && cd fract-anim-test"
Enter
Sleep 500ms

# Show terminal and run init animation
Show
Type "/home/sal/fract/target/release/fract init --path . --color always"
Enter
Sleep 2s

# Clear and run index animation
Type "clear"
Enter
Sleep 500ms
Type "/home/sal/fract/target/release/fract index --color always"
Enter
Sleep 3s

# Screenshot for review
Screenshot fract-refined.png
Sleep 1s

# Show the final report output clearly
Type "clear"
Enter
Sleep 500ms
Type "/home/sal/fract/target/release/fract index --color always"
Enter
Sleep 2s