DejaVuSans.ttf (embedded at compile time for Telegram PNG cards)
================================================================
Source: DejaVu fonts (Bitstream Vera / DejaVu license).

If the file is absent, `build.rs` downloads it with `curl` or `wget`. You can also
fetch manually and commit it for fully offline builds:

  curl -fsSL -o DejaVuSans.ttf \
    https://raw.githubusercontent.com/dejavu-fonts/dejavu-fonts/version_2_37/ttf/DejaVuSans.ttf
