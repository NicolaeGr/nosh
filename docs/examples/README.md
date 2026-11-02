# Examples

Each file has a destination under `~/.config/nosh/`.

| File                  | Copy to                              |
| --------------------- | ------------------------------------ |
| `menus.d/extras.toml` | `~/.config/nosh/menus.d/extras.toml` |
| `apps.d/power.toml`   | `~/.config/nosh/apps.d/power.toml`   |
| `style.css`           | `~/.config/nosh/style.css`           |

`menus.d/extras.toml` defines three menus in one file: power, wallpapers, and themes. Their per-menu stylesheets are still named after the menu ids (`power.css`, `wallpapers.css`, `themes.css`), not after the TOML file.

`apps.d/power.toml` adds Shutdown and Reboot to the app launcher.

`style.css` shows the user stylesheet marker, a color override, and a scoped per-menu rule.

Adjust the wallpaper path and the `ht` command for your own system.
