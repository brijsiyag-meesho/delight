# Working on Delight

## Keep the running app current

The user keeps `cargo run` running in tmux (session `0`, pane `0:0.0`, in
this folder). After finishing a set of changes (once they build), restart it
so the app shows the latest code:

```sh
tmux send-keys -t 0:0.0 C-c
tmux send-keys -t 0:0.0 'cargo run' Enter
```

Then check that it started: `tmux capture-pane -p -J -t 0:0.0 -S -30`
should end with `Running target/debug/delight` and no errors. If the build
fails there, fix it before handing back.

Never kill the app some other way. Test instances you start yourself run
with a scratch `HOME`; stop only those, by the PID you started.
