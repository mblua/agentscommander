---
name: Linux QA webtop container
description: Docker webtop container for Linux QA testing of agentscommander, accessible at localhost:3000
type: reference
---

Container `agentscommander-linux-qa` runs Ubuntu 24.04 XFCE desktop accessible at **http://localhost:3000**.

- Image: `lscr.io/linuxserver/webtop:ubuntu-xfce`
- Volume: `agentscommander-qa-config` mounted at `/config`
- User inside container: `abc` (PUID/PGID 1000)
- Rust/Cargo at: `/config/.cargo/` (installed via rustup for user abc)
- Cloned repo at: `/config/agentscommander/`
- Installed .deb: `agents-commander` v0.4.5 at `/usr/bin/agentscommander`
- Build artifacts: `/config/agentscommander/src-tauri/target/release/bundle/`

To launch the app inside webtop:
```bash
MSYS2_ARG_CONV_EXCL="*" docker exec agentscommander-linux-qa bash -c "DISPLAY=:1 su abc -c 'agentscommander &'"
```

Full setup docs: `docs/LINUX-QA-ENVIRONMENT.md` in the repo.
