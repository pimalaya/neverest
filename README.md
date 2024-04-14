# 📫 Neverest CLI [![release](https://img.shields.io/github/v/release/soywod/neverest?color=success)](https://github.com/soywod/neverest/releases/latest) [![chat](https://img.shields.io/matrix/pimalaya.neverest:matrix.org?color=success&label=chat)](https://matrix.to/#/#pimalaya.neverest:matrix.org)

CLI to synchronize, backup and restore emails, based on [`email-lib`](https://crates.io/crates/email-lib).

![screenshot](https://pimalaya.org/neverest/cli/latest/screenshot.jpeg)

*Disclaimer: the project is under active development, do not use in production before the final `v1.0.0`.*

## Features

- [IMAP](https://pimalaya.org/neverest/cli/latest/configuration/imap.html) support
- [Maildir](https://pimalaya.org/neverest/cli/latest/configuration/maildir.html) and [Notmuch](https://pimalaya.org/neverest/cli/latest/configuration/notmuch.html) support
- Synchronization of two backends together (folders and emails)
- Partial sync based on filters (folders name and envelopes date)
- Restricted sync based on permissions (folder/flag/message create/update/delete)

## Installation

<table>
<tr>
<td width="50%">
<a href="https://repology.org/project/neverest/versions">
<img src="https://repology.org/badge/vertical-allrepos/neverest.svg" alt="Packaging status" />
</a>
</td>
<td width="50%">

```bash
# Cargo
$ cargo install neverest

# Nix
$ nix-env -i neverest
```

*See the [documentation](https://pimalaya.org/neverest/cli/latest/installation.html) for other installation methods.*

</td>
</tr>
</table>

## Configuration

*Please read the [documentation](https://pimalaya.org/neverest/cli/latest/configuration/).*

## Contributing

If you want to **report a bug**, please send an email to the [bug tracker](https://todo.sr.ht/~soywod/pimalaya) at [~soywod/pimalaya@todo.sr.ht](mailto:~soywod/pimalaya@todo.sr.ht).

If you want to **propose a feature** or **fix a bug**, please send a patch to the [mailing list](https://lists.sr.ht/~soywod/pimalaya) at [~soywod/pimalaya@lists.sr.ht](mailto:~soywod/pimalaya@lists.sr.ht). The simplest way to send a patch is to use [git send-email](https://git-scm.com/docs/git-send-email), follow [this guide](https://git-send-email.io/) to configure git properly.

If you just want to **discuss** about the project, feel free to join the [Matrix](https://matrix.org/) workspace [#pimalaya.neverest](https://matrix.to/#/#pimalaya.neverest:matrix.org) or contact me directly [@soywod](https://matrix.to/#/@soywod:matrix.org). You can also [subscribe](mailto:~soywod/pimalaya+subscribe@lists.sr.ht) or [unsubscribe](mailto:~soywod/pimalaya+unsubscribe@lists.sr.ht) to the [mailing list](https://lists.sr.ht/~soywod/pimalaya).

## Sponsoring

[![nlnet](https://nlnet.nl/logo/banner-160x60.png)](https://nlnet.nl/project/Neverest/index.html)

Special thanks to the [NLnet foundation](https://nlnet.nl/project/Neverest/index.html) and the [European Commission](https://www.ngi.eu/) that helped the project to receive financial support from:

- [NGI Assure](https://nlnet.nl/assure/) in 2022
- [NGI Zero Entrust](https://nlnet.nl/entrust/) in 2023

If you appreciate the project, feel free to donate using one of the following providers:

[![GitHub](https://img.shields.io/badge/-GitHub%20Sponsors-fafbfc?logo=GitHub%20Sponsors)](https://github.com/sponsors/soywod)
[![PayPal](https://img.shields.io/badge/-PayPal-0079c1?logo=PayPal&logoColor=ffffff)](https://www.paypal.com/paypalme/soywod)
[![Ko-fi](https://img.shields.io/badge/-Ko--fi-ff5e5a?logo=Ko-fi&logoColor=ffffff)](https://ko-fi.com/soywod)
[![Buy Me a Coffee](https://img.shields.io/badge/-Buy%20Me%20a%20Coffee-ffdd00?logo=Buy%20Me%20A%20Coffee&logoColor=000000)](https://www.buymeacoffee.com/soywod)
[![Liberapay](https://img.shields.io/badge/-Liberapay-f6c915?logo=Liberapay&logoColor=222222)](https://liberapay.com/soywod)
