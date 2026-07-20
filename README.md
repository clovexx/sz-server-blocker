[![CI](https://github.com/clovexx/sz-server-blocker/actions/workflows/ci.yml/badge.svg)](https://github.com/clovexx/sz-server-blocker/actions/workflows/ci.yml)
[![AUR version](https://img.shields.io/aur/version/stalzone-server-blocker?label=AUR)](https://aur.archlinux.org/packages/stalzone-server-blocker)
[![License: MIT](https://img.shields.io/github/license/clovexx/sz-server-blocker)](LICENSE)

# Блокировщик серверов Stalzone

> **Предупреждение:** сетевые утилиты могут противоречить пользовательскому соглашению игры.

TUI для Linux: выборочная блокировка серверов Stalcraft через nftables или iptables.

## Установка

**AUR:**

```bash
paru -S stalzone-server-blocker
```

**Из исходников:**

```bash
git clone https://github.com/clovexx/sz-server-blocker.git
cd sz-server-blocker
cargo build --release
```

## Сборка

```bash
cargo build --release
```

## Использование

```bash
sudo ./target/release/stalzone-server-blocker
```

Применение правил файрвола требует root.

```bash
./target/release/stalzone-server-blocker ping
./target/release/stalzone-server-blocker sync
sudo ./target/release/stalzone-server-blocker apply
sudo ./target/release/stalzone-server-blocker clear
./target/release/stalzone-server-blocker list --blocked
```

### TUI

| Клавиша | Действие |
|---------|----------|
| Пробел | Отметить сервер |
| a, ф | Весь пул |
| r, к | Синхронизация списка |
| b, и | Применить блокировку |
| u, г | Снять блокировку |
| / | Фильтр |
| s, ы | Настройки |
| q, й | Выход |

Поддерживается русская и английская раскладка.

## Список серверов

Список хранится в `~/.config/stalzone-server-blocker/tunnels.txt`. Обновляется из `https://backend.stalcraftx.ru/address_list` — команда `sync` или клавиша `r` в TUI.

```
MSK2-1 - 95.213.255.12 @MSK2
```

## Конфигурация

`~/.config/stalzone-server-blocker/config.json`:

```json
{
  "blocked": ["MSK2-1"],
  "backend": "nftables",
  "apply_on_boot": true,
  "sync_on_startup": true,
  "api_login": "user"
}
```

`backend`: `auto`, `nftables` или `iptables`.  
`apply_on_boot` устанавливает systemd unit: `apply` при загрузке, `clear` при выключении.

## Лицензия

MIT
