# Host auto-deploy

`ops/host-deploy.sh` polls jeryu for the latest `main` pipeline status. When
`main` advances *and* its head pipeline has succeeded, it pulls the new SHA,
rebuilds `jekko-cli` in release mode, and installs the binary to
`~/.local/bin/jekko`.

## One-shot install of the systemd user units

```bash
mkdir -p ~/.config/systemd/user
ln -sf /home/ubuntu/jekko-split/jekko/ops/host-deploy.service ~/.config/systemd/user/host-deploy.service
ln -sf /home/ubuntu/jekko-split/jekko/ops/host-deploy.timer   ~/.config/systemd/user/host-deploy.timer
systemctl --user daemon-reload
systemctl --user enable --now host-deploy.timer
sudo loginctl enable-linger ubuntu  # keeps the user manager alive on logout
```

Verify:

```bash
systemctl --user list-timers host-deploy.timer
systemctl --user status host-deploy.service
tail -f ~/.jekko/host-deploy/deploy.log
```

## Tmux/screen fallback (no systemd)

```bash
tmux new -d -s jekko-deploy 'bash /home/ubuntu/jekko-split/jekko/ops/host-deploy.sh loop'
```

## State files

| File | Purpose |
|---|---|
| `~/.jekko/host-deploy/last-deployed-sha` | SHA most recently installed |
| `~/.jekko/host-deploy/deploy.log` | Append-only log |

## Manual one-shot

```bash
JEKKO_REPO_DIR=/home/ubuntu/jekko-split/jekko bash ops/host-deploy.sh once
```

## Env overrides

| Variable | Default |
|---|---|
| `JERYU_BASE` | `http://localhost:8929` |
| `JERYU_PROJECT_ID` | `148` |
| `JERYU_USER` | `root` |
| `JEKKO_REPO_DIR` | `$HOME/jekko` |
| `JEKKO_INSTALL_ROOT` | `$HOME/.local` |
| `POLL_INTERVAL` | `120` (loop mode only) |
