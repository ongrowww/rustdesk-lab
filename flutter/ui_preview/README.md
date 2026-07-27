# OnGROW UI Preview

Diese kleine Flutter-Web-App rendert ausschließlich die OnGROW-Oberfläche mit
Mock-Daten. Sie benötigt weder den RustDesk-Rust-Build noch vcpkg, native
Bibliotheken oder generierte Flutter-Rust-Bridge-Dateien.

## Starten

Flutter ist für diese Preview mit `mise` auf eine aktuelle stabile Version
gepinnt. Die Installation bleibt auf die Projektumgebung begrenzt:

```bash
cd flutter/ui_preview
mise install
mise exec -- ./run.sh
```

Während der Prozess läuft:

- `r`: Hot Reload
- `R`: Hot Restart
- `q`: Preview beenden

Alternativ kann bei bereits installiertem Flutter direkt `./run.sh` ausgeführt
werden. Das Skript startet einen lokalen Web-Server auf
`http://127.0.0.1:7357` und öffnet die Preview automatisch in Microsoft Edge.

Die Berechtigungsbuttons simulieren erfolgreiche Freigaben. Änderungen an
`../packages/ongrow_support_ui/lib/ongrow_support_view.dart` erscheinen per Hot
Reload in dieser Preview und werden zugleich von der echten Desktop-App
verwendet.
