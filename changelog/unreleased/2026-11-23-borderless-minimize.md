- **A borderless window off macOS can minimize.** `Tx::minimize` and `AppHandle::minimize` used to
  do nothing for a borderless window on Linux and Windows, although winit still reports its
  minimize button and Wayland accepts `set_minimized`. Now they minimize it, as on Wayland's
  `xdg_toplevel`. macOS keeps ignoring borderless windows.
