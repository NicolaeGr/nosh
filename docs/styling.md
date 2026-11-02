# Styling

nosh loads its built-in stylesheet first, then the user stylesheet at `$XDG_CONFIG_HOME/nosh/style.css`.

## The user stylesheet

The file is created on first start and begins with this marker:

```css
/* nosh:builtin-styles */
```

At load time nosh replaces the marker with the complete built-in stylesheet, so those rules are also part of your provider. Remove the marker and the built-in stylesheet still loads from nosh; your file then contains only the rules you wrote.

```css
/* nosh:builtin-styles */

.Bar {
  background: rgba(0, 0, 0, 0.55);
}

.MenuItem {
  border-radius: 6px;
}
```

The built-in rules use GTK named colors such as `@theme_bg_color` and `@accent_bg_color`. You can redefine those colors in the same file:

```css
@define-color accent_bg_color #1e66f5;
```

## Per-menu stylesheets

A menu stylesheet lives at `$XDG_CONFIG_HOME/nosh/menus.d/<id>.css` and is re-read when that menu opens.

The provider is added to the whole display, so scope rules with the menu class `.menu-<id>`:

```css
.menu-wallpapers .MenuItem {
  border-radius: 8px;
}

.menu-wallpapers .MenuItemTitle {
  font-weight: 600;
}
```

The file name is the menu id. One TOML file can define several menus, and each one still gets its own CSS file by id.

## Class names

These are the classes worth targeting.

Bar:

- `.Bar`
- `.BarRoot`
- `.Island`
- `.IslandStart`, `.IslandCenter`, `.IslandEnd`

Menus:

- `.MenuRoot`
- `.MenuSearch`
- `.MenuItem`, `.MenuItemRow`
- `.MenuItemTitle`, `.MenuItemDescription`, `.MenuItemBadge`, `.MenuItemMedia`
- `.MenuItemDisabled`
- `.MenuError`

Notifications:

- `.NotificationsContainer`
- `.NotificationCard`

Quick settings:

- `.QuickSettingsBackground`, `.QuickSettingsCard`
- `.QuickToggle`, `.QuickToggleButton`, `.QuickToggleIcon`, `.QuickToggleName`, `.QuickToggleStatus`
- `.QuickSlider`, `.SliderBlock`, `.SliderIcon`, `.SliderValue`

Centre panel:

- `.CenterPanelBackground`, `.CenterPanelCard`
- `.CenterPanelClock`, `.CenterPanelDate`
- `.CenterPanelMedia`, `.CenterPanelNotificationList`
- `.CalendarGrid`, `.CalendarDay`

Lock screen:

- `.LockBackground`, `.LockCard`
- `.LockClock`, `.LockDate`
- `.LockPassword`, `.LockError`

Power menu and polkit:

- `.PowerMenuBackground`, `.PowerCard`, `.PowerAction`
- `.PolkitBackground`, `.PolkitCard`

The full list is in `crates/nosh-ui/resources/`, split by surface.
