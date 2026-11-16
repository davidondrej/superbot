# Computer: each Bot's screen

Each Bot gets its own screen on the server, with a real browser. You can watch it live in the web app and take over.

## How it works

- **Screen:** a virtual display (Xvfb) with a window manager (Openbox) and its own D-Bus session. `botd` starts it on a Bot's first task and stops it after `SCREEN_IDLE_MINUTES` without use. Code: `server/src/screen.rs`.
- **Browser:** [agent-browser](https://github.com/vercel-labs/agent-browser). Each Bot has its own session and Chrome profile in `DATA_DIR/bots/<id>/browser`, so sign-ins persist. Chrome runs headed on the Bot's screen.
- **Desktop apps:** [agent-desktop](https://github.com/vercel-labs/agent-desktop) reads apps through the screen's accessibility bus.
- **Live view:** [voxl](https://github.com/tembo/voxl) streams each screen. `GET /api/bots/<id>/screen` returns `/screen/<port>/?token=...`. The page is view-only until you click **Take over**.
- **Handoff:** for logins, 2FA, or CAPTCHAs, a Bot runs `botctl handoff "<what it needs>"`. You get a card with **Open screen** and **Done**. The Bot's command waits for your answer.

All tools are plain CLIs, so this works the same on every harness. The Bot's system prompt explains them.

At most `MAX_SCREENS` run at once. When all are in use, the least recently used screen of an idle Bot is stopped.

Screens are work surfaces, not security boundaries. All Bots run as one Linux user.

## Install

On Ubuntu 24.04 or later, as root:

```sh
deploy/install-computer.sh <user that runs botd>
```

Then add `AGENT_BROWSER_SKILLS_DIR=/opt/computer/share/agent-browser-skills` to botd's environment and restart it. It prints `Screens: on.`

The script also adds 4 GB of swap and earlyoom, which kills the biggest Chrome before the server runs out of memory.

## Proxy

The live view needs `/screen/<port>/` routed to that local voxl port. With Caddy:

```
@screen path_regexp screen ^/screen/(69[0-9][0-9])/
handle @screen {
	uri strip_prefix /screen/{re.screen.1}
	reverse_proxy 127.0.0.1:{re.screen.1}
}
```

Ports are `6800 + SCREEN_BASE + slot`: 6901, 6902, ... by default. voxl checks its own token, so the route needs no extra login.

A ready-to-run copy of this, including the route for the app itself, is in [`deploy/Caddyfile`](../deploy/Caddyfile); adjust the port range if you set `SCREEN_BASE`. Without it the panel stays black: the iframe loads `/screen/<port>/` from `botd`, which has no such route.

## Sizing

One Bot with Chrome open uses about 0.5 to 1 GB of RAM. Streaming a busy screen uses about one CPU core. A 2 CPU, 8 GB server handles about 3 screens at once.
