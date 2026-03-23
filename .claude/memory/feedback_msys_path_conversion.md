---
name: MSYS path conversion workaround for docker exec
description: Git Bash on Windows mangles Unix paths in docker exec commands — must use MSYS2_ARG_CONV_EXCL or write scripts into the container
type: feedback
---

When running `docker exec` from Git Bash, MSYS automatically converts Unix paths (e.g., `/tmp/script.sh` becomes `C:/Users/.../Temp/script.sh`), breaking commands inside the container.

**Why:** Git Bash uses MSYS2 which aggressively converts anything that looks like a Unix path to a Windows path before passing it to the command.

**How to apply:**
- Prefix commands with `MSYS2_ARG_CONV_EXCL="*"` to disable all path conversion
- For complex multi-line commands: write a script into the container with `docker exec ... bash -c 'cat > /tmp/script.sh << "EOF" ... EOF'`, then execute it with `MSYS2_ARG_CONV_EXCL="*" docker exec ... /tmp/script.sh`
- `MSYS_NO_PATHCONV=1` works for simple cases but `MSYS2_ARG_CONV_EXCL="*"` is more reliable
- Nested `su - abc -c '...'` is especially prone to double-conversion
