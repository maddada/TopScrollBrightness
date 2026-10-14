# TopScrollBrightness

Move the pointer to the very top edge of a screen and scroll: that monitor's brightness goes up or
down. It changes the monitor's own brightness setting (the one in its on-screen menu and in tools
like ASUS DisplayWidget Center) over DDC/CI, so there's no dimming overlay. Lives in the
notification area.

## Download

Get `TopScrollBrightness.exe` from [Releases](https://github.com/maddada/TopScrollBrightness/releases/latest)
and run it; there's nothing to install. The exe isn't code-signed, so SmartScreen may warn the
first time: click **More info → Run anyway**.

## Using it

| You do | What happens |
|---|---|
| Pointer at the top edge, scroll up | Brighter |
| Pointer at the top edge, scroll down | Dimmer |
| Scroll anywhere else | Normal scrolling |

While you scroll, an indicator styled and placed like Windows 11's volume indicator shows the
level (in your theme and accent colour), then fades out.
Each wheel notch changes the brightness by 5% by default; pick 1, 2, 5 or 10% under **Step per
scroll notch** in the tray menu. High-resolution wheels and trackpads change it smoothly.

Tray icon: **left-click** toggles on/off, **right-click** for the menu (Step per scroll notch,
Start with Windows, Quit). It works with mouse-sharing tools like Deskflow too.

Settings live in `%APPDATA%\TopScrollBrightness\settings.ini`, next to `log.txt`.

## Building

```
cargo build --release
target\release\TopScrollBrightness.exe
```

Needs the MSVC toolchain and the Windows SDK (`rc.exe` embeds the icon, manifest and version
info). `cargo test` runs the brightness, indicator and settings tests.
`TopScrollBrightness.exe --replace` takes over from a copy that's already running.

## Code layout

```
src/
  main.rs              entry point
  shared/              OS-independent
    level.rs           wheel motion → brightness, top-edge test (pure, tested)
    hud.rs             the indicator's layout and artwork, matching Windows 11's volume indicator
    settings.rs        enabled / step, persisted as key = value
    icon.rs            procedural icon (also used by build.rs to make the .ico)
    log.rs             log file + panic hook
  platform/
    mod.rs             what each OS provides: run()
    windows/
      hook.rs          WH_MOUSE_LL hook: wheel on a top edge → brightness change
      brightness.rs    DDC/CI (VCP 0x10) on a worker thread, writing the latest level
      osd.rs           the indicator: a click-through layered window
      tray.rs          notification icon, menu, main message loop
      autostart.rs     Run key
      instance.rs      single-instance lock
build.rs               Windows resources (icon, manifest, version info)
```

Talking to a monitor is slow (a read takes ~70 ms, a write ~55 ms), so the hook only updates the
wanted level and the indicator shows it at once. A worker thread writes the latest level each time
the previous write finishes, skipping the ones in between. After a few seconds without
scrolling, the level is read from the monitor again, so changes made elsewhere are picked up.

## Limitations

- The monitor needs DDC/CI turned on in its menu (most have it on by default). Built-in laptop
  screens don't use DDC/CI and aren't supported.
- Some picture modes lock the brightness (on ASUS OLEDs, typically the HDR modes); the monitor
  then ignores the change.
- DisplayWidget Center may keep showing the old number until you reopen it.
- Only real top edges count: where another monitor continues above, the pointer just passes
  through and scrolling works normally there.

## See also

- [MiddleClickDragScroll](https://github.com/maddada/MiddleClickDragScroll): hold the middle
  mouse button and drag to scroll, with trackpad-style flick momentum.
- [AltMirroredResize](https://github.com/maddada/AltMirroredResize): hold Alt and drag a window
  edge to resize it from both sides at once, like Option-resizing on macOS.
- [Ghostex](https://ghostex.dev) ([GitHub](https://github.com/maddada/Ghostex)): a native
  workspace for coding agents, built in Rust with GPUI.
