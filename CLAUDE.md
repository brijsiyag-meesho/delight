# Working on Delight

## Keep the running app current

The user keeps `cargo run` running in tmux (session `1`, pane `1:0.0`, in
this folder). After finishing a set of changes (once they build), restart it
so the app shows the latest code:

```sh
tmux send-keys -t 1:0.0 C-c
tmux send-keys -t 1:0.0 'cargo run' Enter
```

Then check that it started: `tmux capture-pane -p -J -t 1:0.0 -S -30`
should end with `Running target/debug/delight-app` and no errors. If the build
fails there, fix it before handing back.

Never kill the app some other way. Test instances you start yourself run
with a scratch `HOME`; stop only those, by the PID you started.

## Where code goes

`delight-ui` holds only components more than one place uses (the app and
tools, or several tools). A component only one tool uses lives in that
tool's folder (e.g. the SVG tool's `checkerboard.rs` and `document.rs`);
it moves to `delight-ui` once a second user needs it.
