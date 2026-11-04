//! Eigene Ergänzung im Zweig `eigen`: Wurde die Datei einer offenen Arbeitsmappe inzwischen von
//! außen geändert, zum Beispiel von Syncthing mit der neueren Fassung eines anderen Rechners?
//! Ein normales Speichern würde diese Fassung still ersetzen, und das Sync-Programm verteilt das
//! Ergebnis dann auf alle Rechner. `file.save` verweigert das deshalb, solange niemand mit
//! `"overwrite": true` ausdrücklich zustimmt. Die Oberfläche fragt vorher nach.

/// Was sich eine Arbeitsmappe beim Lesen und Schreiben über ihre Datei merkt. Größe und
/// Änderungszeit lassen sich vor jedem Speichern günstig lesen, und Syncthing übernimmt die
/// Änderungszeit des anderen Rechners mit dem Inhalt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileStamp {
    len: u64,
    modified: std::time::SystemTime,
}

/// Die Fehlermeldung, mit der `file.save` ablehnt. [`changed_elsewhere`] erkennt sie wieder.
pub const CHANGED_ELSEWHERE: &str = "The file changed on disk after it was opened or last saved here, so it was not replaced. Save it under another name, or pass \"overwrite\": true.";

/// Der aktuelle Stand der Datei; `None`, wenn sie fehlt oder nicht lesbar ist.
#[cfg(not(target_arch = "wasm32"))]
pub fn file_stamp(path: &str) -> Option<FileStamp> {
    let meta = std::fs::metadata(path).ok()?;
    Some(FileStamp { len: meta.len(), modified: meta.modified().ok()? })
}

/// Im Browser gibt es keine Dateipfade, also auch nichts zu vergleichen.
#[cfg(target_arch = "wasm32")]
pub fn file_stamp(_path: &str) -> Option<FileStamp> {
    None
}

/// Bedeutet dieser Fehler, dass `file.save` die von außen geänderte Datei nicht ersetzt hat?
pub fn changed_elsewhere(err: &str) -> bool {
    err.contains(CHANGED_ELSEWHERE)
}

impl crate::DocState {
    /// Hat sich die Datei geändert, seit diese Arbeitsmappe sie zuletzt gelesen oder geschrieben
    /// hat? Eine gelöschte Datei zählt nicht: Speichern legt sie neu an, dabei geht nichts verloren.
    pub fn file_changed_elsewhere(&self) -> bool {
        let (Some(path), Some(known)) = (&self.path, self.file_stamp) else { return false };
        file_stamp(path).is_some_and(|now| now != known)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{changed_elsewhere, file_stamp};
    use crate::Session;

    fn ordner(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("gridcraft-file-stamp-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn pfad(dir: &std::path::Path, name: &str) -> String {
        dir.join(name).to_string_lossy().into_owned()
    }

    fn mappe(text: &str, path: &str) {
        let mut s = Session::new();
        s.new_workbook();
        s.run("cell.set", json!({"cell": "A1", "input": text})).unwrap();
        s.run("file.save", json!({"path": path})).unwrap();
    }

    /// Wie Syncthing: neue Datei daneben schreiben, dann über die alte umbenennen.
    fn von_aussen_ersetzen(path: &str, text: &str) {
        let tmp = format!("{path}.fremd.xlsx");
        mappe(text, &tmp);
        std::fs::rename(&tmp, path).unwrap();
    }

    fn a1(path: &str) -> String {
        let mut s = Session::new();
        s.run("file.open", json!({"path": path})).unwrap();
        let r = s.run("cell.get", json!({"cell": "A1"})).unwrap();
        r.to_string()
    }

    fn geoeffnet(path: &str) -> Session {
        let mut s = Session::new();
        s.run("file.open", json!({"path": path})).unwrap();
        s.run("cell.set", json!({"cell": "A1", "input": "Mac B"})).unwrap();
        s
    }

    #[test]
    fn speichern_ersetzt_keine_von_aussen_geaenderte_datei() {
        let dir = ordner("abgelehnt");
        let path = pfad(&dir, "a.xlsx");
        mappe("Alt", &path);
        let mut s = geoeffnet(&path);
        von_aussen_ersetzen(&path, "Mac A");

        assert!(s.active().unwrap().file_changed_elsewhere());
        let e = s.run("file.save", json!({})).unwrap_err().to_string();
        assert!(changed_elsewhere(&e), "{e}");
        assert!(a1(&path).contains("Mac A"), "die Datei bleibt, wie sie ist");
        assert!(s.active().unwrap().is_dirty(), "die Mappe gilt weiter als ungespeichert");
        // Derselbe Pfad, ausdrücklich genannt, ist kein Schlupfloch.
        let e = s.run("file.save", json!({"path": path})).unwrap_err().to_string();
        assert!(changed_elsewhere(&e), "{e}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn nach_der_zustimmung_wird_ueberschrieben_und_nicht_mehr_gefragt() {
        let dir = ordner("zustimmung");
        let path = pfad(&dir, "a.xlsx");
        mappe("Alt", &path);
        let mut s = geoeffnet(&path);
        von_aussen_ersetzen(&path, "Mac A");

        s.run("file.save", json!({"overwrite": true})).unwrap();
        assert!(a1(&path).contains("Mac B"));
        assert!(!s.active().unwrap().is_dirty());
        assert!(!s.active().unwrap().file_changed_elsewhere());
        s.run("cell.set", json!({"cell": "A1", "input": "Mehr"})).unwrap();
        s.run("file.save", json!({})).unwrap();
        assert!(a1(&path).contains("Mehr"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Nur die Änderungszeit weicht ab, die Größe ist gleich.
    #[test]
    fn gleiche_groesse_mit_anderer_zeit_zaehlt_als_aenderung() {
        let dir = ordner("zeit");
        let path = pfad(&dir, "a.xlsx");
        mappe("Alt", &path);
        let s = geoeffnet(&path);
        let vorher = file_stamp(&path).unwrap();
        let file = std::fs::File::options().write(true).open(&path).unwrap();
        file.set_modified(file.metadata().unwrap().modified().unwrap() + std::time::Duration::from_secs(5)).unwrap();
        drop(file);
        assert_ne!(file_stamp(&path).unwrap(), vorher);
        assert!(s.active().unwrap().file_changed_elsewhere());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn normales_arbeiten_loest_nichts_aus() {
        let dir = ordner("normal");
        let path = pfad(&dir, "a.xlsx");
        let mut s = Session::new();
        s.new_workbook();
        assert!(!s.active().unwrap().file_changed_elsewhere(), "noch nie gespeichert");
        s.run("file.save", json!({"path": path})).unwrap();
        for i in 0..3 {
            s.run("cell.set", json!({"cell": "A1", "input": format!("{i}")})).unwrap();
            s.run("file.save", json!({})).unwrap();
        }
        assert!(!s.active().unwrap().file_changed_elsewhere());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Eine CSV-Datei ist die eigene Datei, wird aber nicht zur „vollständig gespeicherten“ Mappe.
    /// Das zweite Speichern darf deshalb nicht das erste für eine fremde Änderung halten.
    #[test]
    fn eine_csv_datei_warnt_nicht_vor_dem_eigenen_speichern() {
        let dir = ordner("csv");
        let path = pfad(&dir, "a.csv");
        std::fs::write(&path, "1,2\n").unwrap();
        let mut s = Session::new();
        s.run("file.open", json!({"path": path})).unwrap();
        s.run("cell.set", json!({"cell": "A1", "input": "7"})).unwrap();
        s.run("file.save", json!({})).unwrap();
        s.run("cell.set", json!({"cell": "A1", "input": "8"})).unwrap();
        s.run("file.save", json!({})).unwrap();
        assert!(std::fs::read_to_string(&path).unwrap().starts_with('8'));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Ein Mac hat die Datei gelöscht: Speichern legt sie wieder an, verloren geht dabei nichts.
    #[test]
    fn eine_geloeschte_datei_wird_neu_angelegt() {
        let dir = ordner("geloescht");
        let path = pfad(&dir, "a.xlsx");
        mappe("Alt", &path);
        let mut s = geoeffnet(&path);
        std::fs::remove_file(&path).unwrap();
        s.run("file.save", json!({})).unwrap();
        assert!(a1(&path).contains("Mac B"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn speichern_unter_neuem_namen_laesst_beide_fassungen_bestehen() {
        let dir = ordner("kopie");
        let (path, kopie) = (pfad(&dir, "a.xlsx"), pfad(&dir, "b.xlsx"));
        mappe("Alt", &path);
        let mut s = geoeffnet(&path);
        von_aussen_ersetzen(&path, "Mac A");
        s.run("file.save", json!({"path": kopie})).unwrap();
        assert!(a1(&path).contains("Mac A"));
        assert!(a1(&kopie).contains("Mac B"));
        assert_eq!(s.active().unwrap().path.as_deref(), Some(kopie.as_str()));
        assert!(!s.active().unwrap().file_changed_elsewhere());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Wer in „Speichern unter“ die Datei selbst auswählt, hat dem Ersetzen schon zugestimmt.
    #[test]
    fn speichern_unter_mit_derselben_datei_ersetzt_sie() {
        let dir = ordner("saveas");
        let path = pfad(&dir, "a.xlsx");
        mappe("Alt", &path);
        let mut s = geoeffnet(&path);
        von_aussen_ersetzen(&path, "Mac A");
        s.run("file.saveAs", json!({"path": path})).unwrap();
        assert!(a1(&path).contains("Mac B"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
