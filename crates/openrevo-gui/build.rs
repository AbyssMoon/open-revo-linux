use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
    CxxQtBuilder::new_qml_module(
        QmlModule::new("org.openrevo").qml_files(["qml/Main.qml", "qml/ModeTile.qml"]),
    )
    .qt_module("Gui")
    .qt_module("Quick")
    .qt_module("QuickControls2")
    .qrc_resources([
        "qml/img/mode_office.svg",
        "qml/img/mode_balanced.svg",
        "qml/img/mode_turbo.svg",
        "qml/img/tray_office.svg",
        "qml/img/tray_balanced.svg",
        "qml/img/tray_turbo.svg",
        "qml/img/tray_unknown.svg",
        "qml/img/charge_high_capacity.svg",
        "qml/img/charge_balanced.svg",
        "qml/img/charge_stationary.svg",
        "qml/img/fan.svg",
        "qml/img/nav_perf.svg",
        "qml/img/nav_perf_on.svg",
        "qml/img/nav_battery.svg",
        "qml/img/nav_battery_on.svg",
        "qml/img/nav_keys.svg",
        "qml/img/nav_keys_on.svg",
        "qml/img/nav_system.svg",
        "qml/img/nav_system_on.svg",
        "qml/img/nav_about.svg",
        "qml/img/nav_about_on.svg",
    ])
    .files(["src/controller.rs"])
    .build();
}
