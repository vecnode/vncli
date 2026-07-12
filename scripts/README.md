
# vncli CLI

`scripts/ubuntu22` = host/orchestration CLI for Ubuntu 22.04.  
`scripts/win11` = host/orchestration CLI for Windows 11.  
`scripts/tools-cli/alpine` = in-container tools workflow CLI for the Alpine Docker image.

### Run

```bat
# Ubuntu 22.04
./scripts/ubuntu22/main.sh

# Windows 11
.\scripts\win11\main.bat

# Alpine container Tools CLI
bash /app/scripts/tools-cli/alpine/main.sh
```

### CLI Dependencies

- curl
- git
- jq
- docker
- rustscan (open-port scanning; `cargo install rustscan`)

Docker Alpine Dependencies

- pandoc
- python
- yt-dlp

