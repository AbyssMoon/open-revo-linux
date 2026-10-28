//! openrevo-gui — графический интерфейс (Qt 6 / QML).

mod controller;
mod single_instance;

use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QUrl};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let has = |a: &str| args.iter().any(|x| x == a);
    // Отладочные снимки окна не мешают запущенному интерфейсу.
    if !has("--screenshot") {
        if let single_instance::Instance::Secondary = single_instance::acquire(!has("--tray")) {
            return;
        }
    }

    // Интерфейс рассчитан на Fusion; остальные стили в переносную сборку не входят.
    if std::env::var_os("QT_QUICK_CONTROLS_STYLE").is_none() {
        std::env::set_var("QT_QUICK_CONTROLS_STYLE", "Fusion");
    }
    // Тема KDE (KDEPlasmaPlatformTheme) делает значок в трее через
    // KStatusNotifierItem, а тот создаёт QMenu — без QApplication это abort.
    // Она подхватывается, если Qt системный (сборка для разработки), причём и
    // через портальную тему: та берёт тему рабочего стола как базовую. Встроенная
    // тема `generic` плагинов не грузит, трей у неё — QDBusTrayIcon самого Qt
    // (тот же протокол StatusNotifierItem), виджеты не нужны.
    std::env::set_var("QT_QPA_PLATFORMTHEME", "generic");

    let mut app = QGuiApplication::new();
    let mut engine = QQmlApplicationEngine::new();

    if let Some(engine) = engine.as_mut() {
        engine.load(&QUrl::from("qrc:/qt/qml/org/openrevo/qml/Main.qml"));
    }

    if let Some(app) = app.as_mut() {
        app.exec();
    }
    single_instance::release();
}
