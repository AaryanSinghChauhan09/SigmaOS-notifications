# Notification Center — Specification

Inspired by omacom/omarchy-notification-center-plugin.

- D-Bus interface `org.freedesktop.Notifications` compatible
- Actions: default, dismiss; hints: urgency, image-path, resident
- Persistence: JSON-lines store under `$XDG_STATE_HOME/sigmaos/notifications/`
- Do-not-disturb + per-app rules in `notifications.toml`
