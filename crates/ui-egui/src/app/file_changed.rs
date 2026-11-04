//! Eigene Ergänzung im Zweig `eigen`: Die Nachfrage, bevor Speichern eine von außen geänderte
//! Datei ersetzt (siehe `gridcraft_engine::file_stamp`). Die deutschen Texte stehen in `i18n/de.rs`.

use super::{Json, SheetApp, json};
use crate::dialogs::Dialog;

impl SheetApp {
    /// Muss `file.save` mit diesen Parametern erst nachfragen? Nur auf die eigene Datei der aktiven
    /// Mappe, und nur ohne ausdrückliche Zustimmung.
    pub(super) fn save_needs_asking(&self, params: &Json) -> bool {
        params.get("overwrite").and_then(Json::as_bool) != Some(true)
            && self
                .session
                .active()
                .is_some_and(|d| params.get("path").and_then(Json::as_str).is_none_or(|p| d.path.as_deref() == Some(p)) && d.file_changed_elsewhere())
    }

    /// Zeigt die Nachfrage für die aktive Mappe.
    pub(super) fn ask_file_changed(&mut self) {
        let title = self.session.active().map(|d| d.display_title()).unwrap_or_default();
        self.open_dialog("fileChanged", json!({"title": title}));
        self.wake_for_file_changed();
    }

    /// Weckt das Fenster für die Nachfrage: Sie darf nicht auf die nächste Eingabe warten, und
    /// AutoSpeichern läuft auch, während das Fenster verdeckt ist und nichts gezeichnet wird.
    fn wake_for_file_changed(&self) {
        if let Some(ctx) = &self.file_changed_ctx {
            let viewport = ctx.viewport_id();
            if ctx.input_for(viewport, |i| i.viewport().visible()) != Some(true) {
                ctx.send_viewport_cmd_to(viewport, egui::ViewportCommand::RequestPaintWhileHidden);
            }
            ctx.request_repaint_of(viewport);
            ctx.request_repaint();
        }
    }

    /// Soll AutoSave aussetzen, weil die Datei von außen geändert wurde? AutoSave schreibt dann
    /// nichts und fragt einmal nach. Danach bleibt es still, bis die Mappe gespeichert wurde.
    /// Ein anderer offener Dialog bleibt stehen; dann fragt der nächste Durchlauf.
    pub(super) fn autosave_stopped_by_changed_file(&mut self) -> bool {
        let Some((uid, title, changed)) = self.session.active().map(|d| (d.uid, d.display_title(), d.file_changed_elsewhere())) else {
            return false;
        };
        if !changed {
            self.file_changed_warned = None;
            return false;
        }
        if self.file_changed_warned != Some(uid) && self.dialog.is_none() {
            self.file_changed_warned = Some(uid);
            self.open_dialog("fileChanged", json!({"title": title, "autosave": true}));
            self.wake_for_file_changed();
        }
        true
    }
}

/// Der Dialog: Unter neuem Namen speichern behält beide Fassungen, Überschreiben ersetzt die
/// Datei, Abbrechen und Esc lassen alles, wie es ist.
pub(crate) fn file_changed_body(app: &mut SheetApp, ui: &mut egui::Ui, d: &Dialog, open: &mut bool) {
    let lang = crate::i18n::current(ui.ctx());
    let name = d.values.get("title").and_then(Json::as_str).unwrap_or_default();
    ui.label(egui::RichText::new(lang.tr("{name} has changed in the meantime").replace("{name}", name)).strong());
    ui.label(lang.tr(
        "The file was changed from outside since you opened or last saved it, for example on another computer. If you overwrite it, your version replaces that change.",
    ));
    ui.label(lang.tr("Saving under a new name keeps both versions, so you can compare them."));
    if d.values.get("autosave").and_then(Json::as_bool) == Some(true) {
        ui.label(lang.tr("AutoSave is paused for this workbook until you save it."));
    }
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(lang.tr("Cancel")).clicked() {
                *open = false;
            }
            if ui.button(lang.tr("Overwrite")).clicked() {
                *open = false;
                app.run_or_alert("file.save", json!({"overwrite": true}));
            }
            let t = crate::theme::Tokens::get(ui.ctx());
            let save_as = egui::Button::new(egui::RichText::new(lang.tr("Save Under a New Name…")).color(t.on_accent)).fill(t.accent);
            if ui.add(save_as).clicked() {
                *open = false;
                app.open_dialog("saveAs", json!({}));
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use gridcraft_engine::Session;
    use serde_json::json;

    use crate::{SaveAnswer, Services, SheetApp};

    fn ordner(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("gridcraft-ui-changed-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
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
        s.run("cell.get", json!({"cell": "A1"})).unwrap().to_string()
    }

    fn app() -> SheetApp {
        let mut session = Session::new();
        session.new_workbook();
        SheetApp::new(session, Services::default())
    }

    /// Eine geöffnete Mappe mit Änderung, deren Datei inzwischen von außen ersetzt wurde.
    fn veraltet(dir: &std::path::Path) -> (SheetApp, String) {
        let path = dir.join("a.xlsx").to_string_lossy().into_owned();
        mappe("Alt", &path);
        let mut a = app();
        a.open_path(&path);
        von_aussen_ersetzen(&path, "Mac A");
        a.run("cell.set", json!({"cell": "A1", "input": "Mac B"})).unwrap();
        (a, path)
    }

    fn dialog(a: &SheetApp) -> Option<&str> {
        a.dialog.as_ref().map(|d| d.name())
    }

    fn autosave_faellig(a: &mut SheetApp) {
        a.ui.autosave = true;
        a.grid.last_autosave = crate::monotonic_ms() - 3000.0;
        a.autosave(&egui::Context::default());
    }

    #[test]
    fn speichern_fragt_nach_und_laesst_die_datei_in_ruhe() {
        let dir = ordner("fragt");
        let (mut a, path) = veraltet(&dir);
        a.run_or_alert("file.save", json!({}));
        assert_eq!(dialog(&a), Some("fileChanged"));
        assert!(a.message.is_none(), "kein zusätzliches Meldungsfenster");
        assert!(a1(&path).contains("Mac A"));
        assert!(a.session.active().unwrap().is_dirty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Skripte und Agenten bekommen nie einen Dialog, sondern einen Fehler.
    #[test]
    fn skripte_bekommen_einen_fehler() {
        let dir = ordner("skripte");
        let (mut a, path) = veraltet(&dir);
        let e = a.run("file.save", json!({})).unwrap_err();
        assert!(gridcraft_engine::file_stamp::changed_elsewhere(&e), "{e}");
        assert_eq!(dialog(&a), None);
        assert!(a1(&path).contains("Mac A"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn autosave_ersetzt_nichts_und_fragt_einmal() {
        let dir = ordner("autosave");
        let (mut a, path) = veraltet(&dir);
        autosave_faellig(&mut a);
        assert!(a1(&path).contains("Mac A"), "AutoSave ersetzt die Datei nicht");
        assert_eq!(dialog(&a), Some("fileChanged"));

        // Abgebrochen: Weitere Durchläufe schreiben nichts und fragen nicht erneut.
        a.dialog = None;
        autosave_faellig(&mut a);
        assert_eq!(dialog(&a), None);
        assert!(a1(&path).contains("Mac A"));

        // Nach dem Überschreiben läuft AutoSave wieder.
        a.run_or_alert("file.save", json!({"overwrite": true}));
        assert!(a1(&path).contains("Mac B"));
        a.run("cell.set", json!({"cell": "A1", "input": "Mehr"})).unwrap();
        autosave_faellig(&mut a);
        assert!(a1(&path).contains("Mehr"));
        assert!(!a.session.active().unwrap().is_dirty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Ein anderer offener Dialog wird nicht ersetzt, die Nachfrage kommt danach.
    #[test]
    fn autosave_verdraengt_keinen_offenen_dialog() {
        let dir = ordner("anderer-dialog");
        let (mut a, path) = veraltet(&dir);
        a.open_dialog("start", json!({}));
        autosave_faellig(&mut a);
        assert_eq!(dialog(&a), Some("start"));
        assert!(a1(&path).contains("Mac A"));
        a.dialog = None;
        autosave_faellig(&mut a);
        assert_eq!(dialog(&a), Some("fileChanged"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Beim Schließen: Speichern führt zur Nachfrage, die Mappe bleibt offen, nichts wird ersetzt.
    #[test]
    fn schliessen_mit_speichern_fragt_und_bleibt_offen() {
        let dir = ordner("schliessen");
        let (mut a, path) = veraltet(&dir);
        let dokumente = a.session.documents().len();
        assert!(a.answer_save_changes(SaveAnswer::Save, true, &egui::Context::default()));
        assert_eq!(dialog(&a), Some("fileChanged"));
        assert_eq!(a.session.documents().len(), dokumente);
        assert!(!a.allow_close, "die App schließt noch nicht");
        assert!(a1(&path).contains("Mac A"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn fenster(a: SheetApp) -> egui_kittest::Harness<'static, SheetApp> {
        let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1440.0, 900.0)).build_ui_state(
            |ui, app: &mut SheetApp| {
                let ctx = ui.ctx().clone();
                app.logic(&ctx);
                app.ui(ui);
            },
            a,
        );
        for _ in 0..4 {
            h.step();
        }
        h
    }

    fn schritte(h: &mut egui_kittest::Harness<'static, SheetApp>) {
        for _ in 0..3 {
            h.step();
        }
    }

    /// Der Dialog im echten Fenster: Überschreiben ersetzt die Datei.
    #[test]
    fn der_knopf_ueberschreiben_ersetzt_die_datei() {
        use egui_kittest::kittest::Queryable;
        let dir = ordner("fenster-ueberschreiben");
        let (a, path) = veraltet(&dir);
        let mut h = fenster(a);
        h.state_mut().run_or_alert("file.save", json!({}));
        schritte(&mut h);
        h.get_by_label("Overwrite").click();
        schritte(&mut h);
        assert_eq!(dialog(h.state()), None);
        assert!(a1(&path).contains("Mac B"));
        assert!(!h.state().session.active().unwrap().is_dirty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Der Dialog im echten Fenster: Abbrechen lässt alles, wie es ist.
    #[test]
    fn der_knopf_abbrechen_laesst_alles_wie_es_ist() {
        use egui_kittest::kittest::Queryable;
        let dir = ordner("fenster-abbrechen");
        let (a, path) = veraltet(&dir);
        let mut h = fenster(a);
        h.state_mut().run_or_alert("file.save", json!({}));
        schritte(&mut h);
        // Die Bearbeitungsleiste hat auch einen Knopf „Cancel“; der des Dialogs kommt zuletzt.
        h.query_all_by_label("Cancel").last().unwrap().click();
        schritte(&mut h);
        assert_eq!(dialog(h.state()), None);
        assert!(a1(&path).contains("Mac A"));
        assert!(h.state().session.active().unwrap().is_dirty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Der Dialog im echten Fenster: Ein neuer Name behält beide Fassungen.
    #[test]
    fn der_knopf_neuer_name_behaelt_beide_fassungen() {
        use egui_kittest::kittest::Queryable;
        let dir = ordner("fenster-name");
        let (mut a, path) = veraltet(&dir);
        let kopie = dir.join("b.xlsx").to_string_lossy().into_owned();
        let gewaehlt = kopie.clone();
        a.services.pick_save = Some(Box::new(move |_| Some(gewaehlt.clone())));
        let mut h = fenster(a);
        h.state_mut().run_or_alert("file.save", json!({}));
        schritte(&mut h);
        h.get_by_label("Save Under a New Name…").click();
        schritte(&mut h);
        assert_eq!(dialog(h.state()), None);
        assert!(a1(&path).contains("Mac A"), "die Fassung von Mac A bleibt");
        assert!(a1(&kopie).contains("Mac B"), "die eigene Fassung liegt daneben");
        assert_eq!(h.state().session.active().unwrap().path.as_deref(), Some(kopie.as_str()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Führt `tue` aus, nachdem das Fenster zur Ruhe gekommen ist, und sagt, ob es dabei sofort zum
    /// Neuzeichnen geweckt wurde.
    fn weckt_das_fenster(h: &mut egui_kittest::Harness<'static, SheetApp>, tue: impl FnOnce(&mut SheetApp, &egui::Context)) -> bool {
        for _ in 0..10 {
            h.step();
        }
        let geweckt = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let merker = geweckt.clone();
        h.ctx.set_request_repaint_callback(move |info| {
            if info.delay.is_zero() {
                merker.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        });
        let ctx = h.ctx.clone();
        tue(h.state_mut(), &ctx);
        geweckt.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Die Warnung wartet nicht auf die nächste Mausbewegung: Das Fenster wird von selbst geweckt.
    #[test]
    fn speichern_weckt_das_fenster_fuer_die_warnung() {
        let dir = ordner("wecken-speichern");
        let (a, _) = veraltet(&dir);
        let mut h = fenster(a);
        let geweckt = weckt_das_fenster(&mut h, |a, _| a.run_or_alert("file.save", json!({})));
        assert_eq!(dialog(h.state()), Some("fileChanged"));
        assert!(geweckt, "die Warnung muss ohne weitere Eingabe erscheinen");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// AutoSpeichern läuft auch bei verdecktem Fenster: Auch dann wird die Warnung gezeichnet.
    #[test]
    fn autosave_weckt_das_fenster_fuer_die_warnung() {
        let dir = ordner("wecken-autosave");
        let (a, _) = veraltet(&dir);
        let mut h = fenster(a);
        let geweckt = weckt_das_fenster(&mut h, |a, ctx| {
            a.ui.autosave = true;
            a.grid.last_autosave = crate::monotonic_ms() - 3000.0;
            a.autosave(ctx);
        });
        assert_eq!(dialog(h.state()), Some("fileChanged"));
        assert!(geweckt, "die Warnung muss ohne weitere Eingabe erscheinen");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn die_texte_gibt_es_auf_deutsch() {
        let de = crate::i18n::Language::De;
        assert_eq!(de.tr("Overwrite"), "Überschreiben");
        assert_eq!(de.tr("Save Under a New Name…"), "Unter neuem Namen speichern…");
        assert_eq!(de.tr("{name} has changed in the meantime"), "„{name}“ wurde inzwischen geändert");
        assert_ne!(de.tr("AutoSave is paused for this workbook until you save it."), "AutoSave is paused for this workbook until you save it.");
    }
}
