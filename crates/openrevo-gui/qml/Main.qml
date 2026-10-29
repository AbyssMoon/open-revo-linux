import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Qt.labs.platform as Platform

import org.openrevo

ApplicationWindow {
    id: win

    width: 760
    height: 620
    minimumWidth: 560
    minimumHeight: 420
    // --tray: старт свёрнутым в трей (автозапуск при входе).
    visible: Qt.application.arguments.indexOf("--tray") < 0
    title: "Open Revo"

    Controller {
        id: ctl
        onShowRequested: win.showWindow()
    }

    // --- Трей ---
    property bool quitting: false

    function showWindow() {
        win.show();
        win.raise();
        win.requestActivate();
    }

    onClosing: close => {
        if (tray.available && !win.quitting && win.argValue("--screenshot") === "") {
            close.accepted = false;
            win.hide();
        }
    }

    // Строки состояния — в подсказке и в меню трея.
    readonly property string trayModeLine: ctl.connected ? "Режим: " + (win.modeTitle[ctl.perfMode] || "?")
                                                         : "Нет связи с демоном"
    readonly property string trayCpuLine: ctl.cpuTemp < 0 ? "" :
        "CPU: " + (ctl.cpuPower >= 0 ? ctl.cpuPower.toFixed(1) + " Вт · " : "")
        + (ctl.cpuFreq > 0 ? (ctl.cpuFreq / 1000).toFixed(1) + " ГГц · " : "")
        + ctl.cpuTemp + " °C"
    readonly property string trayBatteryLine: {
        if (ctl.batteryCapacity < 0)
            return "";
        let s = "Батарея: " + ctl.batteryCapacity + "%";
        const p = ctl.batteryPower;
        if (Math.abs(p) >= 0.1)
            s += " · " + (p > 0 ? "+" : "−") + Math.abs(p).toFixed(1) + " Вт";
        if (win.chargeUpTo < 100)
            s += " · заряд до " + win.chargeUpTo + "%";
        else if (!win.hasChargeLimit && ctl.chargeProfile !== "" && ctl.chargeProfile !== "high_capacity")
            s += " · " + win.chargeTitle[ctl.chargeProfile].toLowerCase();
        return s;
    }

    Platform.SystemTrayIcon {
        id: tray
        visible: available
        icon.source: Qt.resolvedUrl("img/tray_" + (ctl.connected && ctl.perfMode ? ctl.perfMode : "unknown") + ".svg")
        tooltip: ["Open Revo", win.trayModeLine, win.trayCpuLine, win.trayBatteryLine]
                 .filter(l => l !== "").join("\n")

        onActivated: reason => {
            if (reason === Platform.SystemTrayIcon.Trigger) {
                if (win.visible)
                    win.hide();
                else
                    win.showWindow();
            }
        }

        menu: Platform.Menu {
            Platform.MenuItem {
                enabled: false
                text: win.trayModeLine
            }
            Platform.MenuItem {
                enabled: false
                visible: text !== ""
                text: win.trayCpuLine
            }
            Platform.MenuItem {
                enabled: false
                visible: text !== ""
                text: win.trayBatteryLine
            }
            Platform.MenuSeparator {}

            Platform.MenuItemGroup { id: trayModes }
            Platform.MenuItem {
                text: "Офис"; checkable: true; group: trayModes
                checked: ctl.perfMode === "office"; enabled: win.canSwitchModes
                onTriggered: ctl.setPerf("office")
            }
            Platform.MenuItem {
                text: "Баланс"; checkable: true; group: trayModes
                checked: ctl.perfMode === "balanced"; enabled: win.canSwitchModes
                onTriggered: ctl.setPerf("balanced")
            }
            Platform.MenuItem {
                text: "Турбо"; checkable: true; group: trayModes
                checked: ctl.perfMode === "turbo"; enabled: win.canSwitchModes
                onTriggered: ctl.setPerf("turbo")
            }
            Platform.MenuSeparator {}
            Platform.MenuItem {
                text: "Вентиляторы на максимум"; checkable: true
                visible: ctl.fanMax >= 0
                checked: ctl.fanMax === 1; enabled: ctl.connected
                onTriggered: ctl.requestFanMax(ctl.fanMax !== 1)
            }
            Platform.MenuItem {
                text: "Подсветка клавиатуры"; checkable: true
                visible: ctl.kbdBrightness >= 0
                checked: ctl.kbdBrightness > 0; enabled: ctl.connected
                onTriggered: ctl.requestKbdBrightness(ctl.kbdBrightness > 0 ? 0
                                                      : (win.kbdOnLevel || Math.ceil(ctl.kbdMax / 2)))
            }
            Platform.MenuItem {
                text: "Fn Lock"; checkable: true
                visible: ctl.fnLock >= 0
                checked: ctl.fnLock === 1; enabled: ctl.connected
                onTriggered: ctl.requestFnLock(ctl.fnLock !== 1)
            }
            Platform.MenuItem {
                text: win.visible ? "Скрыть окно" : "Открыть окно"
                onTriggered: win.visible ? win.hide() : win.showWindow()
            }
            Platform.MenuItem {
                text: "Выход"
                onTriggered: {
                    win.quitting = true;
                    Qt.quit();
                }
            }
        }
    }

    // Отладка: openrevo-gui --page N --screenshot файл.png
    function argValue(name) {
        const a = Qt.application.arguments;
        const i = a.indexOf(name);
        return i >= 0 && i + 1 < a.length ? a[i + 1] : "";
    }

    Timer {
        interval: 800
        running: win.argValue("--screenshot") !== ""
        onTriggered: body.grabToImage(r => {
            r.saveToFile(win.argValue("--screenshot"));
            Qt.quit();
        })
    }

    Component.onCompleted: {
        const page = win.argValue("--page");
        if (page !== "")
            nav.currentIndex = parseInt(page);
        // Без трея скрытое окно было бы не открыть.
        if (!win.visible && !tray.available)
            win.showWindow();
        ctl.listenInstances();
        ctl.listenEvents();
    }

    Timer {
        // Питание приходит событием (listenEvents); мощность, частота,
        // температура и вентиляторы меняются сами.
        interval: win.visible ? 2000 : 5000
        running: true
        repeat: true
        triggeredOnStart: true
        onTriggered: ctl.refresh()
    }

    readonly property bool canSwitchModes: ctl.connected && ctl.profilesAvailable >= 3

    readonly property var modeTitle: ({ office: "Офис", balanced: "Баланс", turbo: "Турбо" })
    readonly property var modeColor: ({ office: "#38bdf8", balanced: "#34d399", turbo: "#fbbf24" })
    readonly property var modeHint: ({
        office: "Тише и холоднее, дольше от батареи",
        balanced: "Обычная работа",
        turbo: "Максимум мощности, вентиляторы громче"
    })
    // Замеры для этой модели: {режим: [длительный, кратковременный]}, Вт.
    readonly property var modeLimits: JSON.parse(ctl.modeLimits || "{}")

    readonly property var chargeTitle: ({
        high_capacity: "Полный заряд", balanced: "Баланс", stationary: "Всегда от сети"
    })
    readonly property var chargeColor: ({
        high_capacity: "#fbbf24", balanced: "#34d399", stationary: "#38bdf8"
    })
    readonly property var chargeHint: ({
        high_capacity: "Максимум времени работы без розетки — например, в дорогу",
        balanced: "Хватит на обычный день, а батарея изнашивается медленнее",
        stationary: "Ноутбук почти всегда подключён к сети — батарея прослужит дольше всего"
    })
    // До скольких процентов заряжает профиль (если драйвер умеет лимит заряда).
    readonly property var chargeLimitOf: ({ high_capacity: 100, balanced: 80, stationary: 60 })
    readonly property bool hasChargeLimit: ctl.chargeLimit >= 0
    // До скольких процентов батарея зарядится сейчас.
    readonly property int chargeUpTo: win.hasChargeLimit && ctl.chargeLimitActive
                                      && ctl.chargeLimit > 0 && ctl.chargeLimit < 100 ? ctl.chargeLimit : 100

    // Подсветка: на какой уровень включать (последний ненулевой).
    property int kbdOnLevel: 0
    Connections {
        target: ctl
        function onKbdBrightnessChanged() {
            if (ctl.kbdBrightness > 0)
                win.kbdOnLevel = ctl.kbdBrightness;
        }
    }
    readonly property var kbdColors: [
        { name: "Белый", c: "#ffffff" }, { name: "Красный", c: "#ff0000" },
        { name: "Оранжевый", c: "#ff6000" }, { name: "Жёлтый", c: "#ffc800" },
        { name: "Зелёный", c: "#00ff00" }, { name: "Бирюзовый", c: "#00ffff" },
        { name: "Синий", c: "#0000ff" }, { name: "Фиолетовый", c: "#a000ff" }
    ]

    readonly property var pages: [
        { title: "Производительность", icon: "perf" },
        { title: "Батарея", icon: "battery" },
        { title: "Клавиатура", icon: "keys" },
        { title: "Система", icon: "system" },
        { title: "О программе", icon: "about" }
    ]

    header: ToolBar {
        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: 12
            anchors.rightMargin: 12

            Label {
                text: "Open Revo"
                font.pixelSize: 18
                font.bold: true
            }
            Item { Layout.fillWidth: true }
            Rectangle {
                width: 10
                height: 10
                radius: 5
                color: ctl.connected ? "#3cb371" : "#d9534f"
            }
            Label {
                text: ctl.connected ? ctl.backend : "нет связи с демоном"
                elide: Text.ElideRight
                Layout.maximumWidth: 320
            }
        }
    }

    Pane {
        id: body
        anchors.fill: parent
        padding: 0

        ColumnLayout {
            anchors.fill: parent
            spacing: 0

            // Предупреждения
            Pane {
                Layout.fillWidth: true
                visible: warning.text.length > 0
                background: Rectangle { color: "#fff3cd" }

                Label {
                    id: warning
                    width: parent.width
                    wrapMode: Text.Wrap
                    color: "#664d03"
                    text: {
                        if (!ctl.connected)
                            return ctl.errorText + "\nДемон не запущен: sudo systemctl start openrevod";
                        let s = ctl.actionError;
                        if (ctl.errorText)
                            s = (s ? s + "\n" : "") + ctl.errorText;
                        if (!ctl.isMechrevo && !ctl.backend.startsWith("mock"))
                            s = (s ? s + "\n" : "") + "Это не ноутбук MECHREVO — проверено только на WUJIE 15X.";
                        return s;
                    }
                }
            }

            RowLayout {
                Layout.fillWidth: true
                Layout.fillHeight: true
                spacing: 0

                // Меню разделов.
                Rectangle {
                    Layout.preferredWidth: 232
                    Layout.fillHeight: true
                    color: Qt.darker(win.palette.window, 1.04)

                    ListView {
                        id: nav
                        anchors.fill: parent
                        anchors.margins: 8
                        spacing: 2
                        model: win.pages
                        currentIndex: 0
                        focus: true
                        keyNavigationEnabled: true
                        boundsBehavior: Flickable.StopAtBounds

                        delegate: ItemDelegate {
                            id: navItem
                            required property var modelData
                            required property int index
                            readonly property bool current: ListView.isCurrentItem
                            width: ListView.view.width
                            height: 42
                            leftPadding: 12
                            onClicked: nav.currentIndex = index

                            background: Rectangle {
                                radius: 8
                                color: navItem.current ? Qt.rgba(0.18, 0.5, 0.76, 0.14)
                                     : navItem.hovered ? Qt.rgba(0.5, 0.5, 0.5, 0.12)
                                     : "transparent"
                                Behavior on color { ColorAnimation { duration: 120 } }

                                // Полоска у выбранного пункта.
                                Rectangle {
                                    visible: navItem.current
                                    width: 3
                                    height: 20
                                    radius: 1.5
                                    anchors.left: parent.left
                                    anchors.leftMargin: 2
                                    anchors.verticalCenter: parent.verticalCenter
                                    color: "#2f7fc1"
                                }
                            }

                            contentItem: RowLayout {
                                spacing: 12
                                Image {
                                    source: "img/nav_" + navItem.modelData.icon + (navItem.current ? "_on" : "") + ".svg"
                                    sourceSize: Qt.size(20, 20)
                                    Layout.preferredWidth: 20
                                    Layout.preferredHeight: 20
                                }
                                Label {
                                    Layout.fillWidth: true
                                    text: navItem.modelData.title
                                    font.pixelSize: 14
                                    font.weight: navItem.current ? Font.DemiBold : Font.Normal
                                    color: navItem.current ? "#2f7fc1" : win.palette.windowText
                                    elide: Text.ElideRight
                                }
                            }
                        }
                    }
                }

                Rectangle {
                    Layout.preferredWidth: 1
                    Layout.fillHeight: true
                    color: Qt.darker(win.palette.window, 1.15)
                }

                StackLayout {
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    Layout.margins: 16
                    currentIndex: nav.currentIndex

                    // --- Производительность ---
                    ScrollView {
                        id: perfScroll
                        clip: true
                        contentWidth: availableWidth

                    ColumnLayout {
                        width: perfScroll.availableWidth
                        spacing: 14

                        Label {
                            text: "Производительность"
                            font.pixelSize: 16
                            font.bold: true
                        }

                        RowLayout {
                            Layout.fillWidth: true
                            spacing: 10

                            Repeater {
                                model: ["office", "balanced", "turbo"]

                                ModeTile {
                                    required property string modelData
                                    readonly property string mode: modelData
                                    readonly property var limits: win.modeLimits[mode]
                                    Layout.fillWidth: true
                                    Layout.preferredWidth: 1
                                    imageSize: 64
                                    image: "img/mode_" + mode + ".svg"
                                    caption: win.modeTitle[mode]
                                    subtitle: limits ? "до " + limits[0] + " Вт" : ""
                                    accent: win.modeColor[mode]
                                    checked: ctl.perfMode === mode
                                    enabled: win.canSwitchModes
                                    ToolTip.visible: hovered
                                    ToolTip.delay: 400
                                    ToolTip.text: win.modeHint[mode] + (limits
                                        ? "\nCPU от сети: " + limits[0] + " Вт, первые ~50 с — до " + limits[1] + " Вт"
                                        : "")
                                    // Не даём кнопке переключиться до ответа демона.
                                    onClicked: {
                                        checked = Qt.binding(() => ctl.perfMode === mode);
                                        ctl.setPerf(mode);
                                    }
                                }
                            }
                        }

                        Label {
                            visible: ctl.connected && ctl.perfRaw >= 0 && ctl.perfMode === ""
                            text: "EC сообщает режим без названия: 0x" + ctl.perfRaw.toString(16).padStart(2, "0")
                        }

                        // Карточка «сейчас».
                        Rectangle {
                            Layout.fillWidth: true
                            implicitHeight: nowCol.implicitHeight + 28
                            radius: 12
                            color: "#1f1f1f"
                            visible: ctl.cpuTemp >= 0 || ctl.cpuPower >= 0

                            ColumnLayout {
                                id: nowCol
                                anchors.fill: parent
                                anchors.margins: 14
                                spacing: 12

                                Row {
                                    id: metrics
                                    Layout.fillWidth: true

                                    Repeater {
                                        model: [
                                            { v: ctl.cpuPower >= 0 ? ctl.cpuPower.toFixed(1) : "—", u: "Вт", l: "потребление CPU" },
                                            { v: ctl.cpuFreq > 0 ? (ctl.cpuFreq / 1000).toFixed(2) : "—", u: "ГГц", l: "частота" },
                                            { v: ctl.cpuTemp >= 0 ? ctl.cpuTemp : "—", u: "°C", l: "температура",
                                              hot: ctl.cpuTemp >= 95 }
                                        ]
                                        Column {
                                            required property var modelData
                                            width: metrics.width / 3
                                            Label {
                                                anchors.horizontalCenter: parent.horizontalCenter
                                                textFormat: Text.StyledText
                                                text: "<b>" + modelData.v + "</b> <small>" + modelData.u + "</small>"
                                                color: modelData.hot ? "#f43f5e" : "white"
                                                font.pixelSize: 24
                                            }
                                            Label {
                                                anchors.horizontalCenter: parent.horizontalCenter
                                                text: modelData.l
                                                color: "#8a8a8a"
                                                font.pixelSize: 11
                                            }
                                        }
                                    }
                                }

                                // Вентиляторы.
                                RowLayout {
                                    Layout.fillWidth: true
                                    visible: ctl.fan1 >= 0
                                    spacing: 16

                                    // Модель — число, а не массив со значениями: массив
                                    // собирался бы заново на каждом обновлении, строки
                                    // пересоздавались бы, и полоска росла бы от нуля.
                                    Repeater {
                                        model: 2
                                        RowLayout {
                                            id: fanRow
                                            required property int index
                                            readonly property int v: index === 0 ? ctl.fan1 : ctl.fan2
                                            readonly property string n: index === 0 ? "CPU" : "второй"
                                            Layout.fillWidth: true
                                            visible: v >= 0
                                            spacing: 8

                                            Image {
                                                source: "img/fan.svg"
                                                sourceSize: Qt.size(22, 22)
                                                // Крутится, пока вентилятор работает.
                                                RotationAnimation on rotation {
                                                    from: 0; to: 360
                                                    duration: Math.max(300, 3000 - fanRow.v * 25)
                                                    loops: Animation.Infinite
                                                    running: fanRow.v > 0 && win.visible
                                                }
                                            }
                                            Label {
                                                text: "вентилятор " + fanRow.n
                                                color: "#8a8a8a"
                                                font.pixelSize: 12
                                            }
                                            Rectangle {
                                                Layout.fillWidth: true
                                                height: 6
                                                radius: 3
                                                color: "#333333"
                                                Rectangle {
                                                    width: parent.width * fanRow.v / 100
                                                    height: parent.height
                                                    radius: 3
                                                    color: win.modeColor[ctl.perfMode] || "#9ca3af"
                                                    Behavior on width { NumberAnimation { duration: 300 } }
                                                }
                                            }
                                            Label {
                                                Layout.preferredWidth: 38
                                                horizontalAlignment: Text.AlignRight
                                                text: fanRow.v > 0 ? fanRow.v + "%" : "стоит"
                                                color: "white"
                                                font.pixelSize: 12
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        RowLayout {
                            Layout.fillWidth: true
                            visible: ctl.fanMax >= 0
                            spacing: 10

                            Switch {
                                enabled: ctl.connected
                                checked: ctl.fanMax === 1
                                onToggled: {
                                    const want = checked;
                                    checked = Qt.binding(() => ctl.fanMax === 1);
                                    ctl.requestFanMax(want);
                                }
                            }
                            ColumnLayout {
                                Layout.fillWidth: true
                                spacing: 0
                                Label {
                                    text: "Вентиляторы на максимум"
                                    font.bold: true
                                }
                                Label {
                                    Layout.fillWidth: true
                                    wrapMode: Text.Wrap
                                    opacity: 0.65
                                    font.pixelSize: 12
                                    text: ctl.fanMax === 1
                                        ? "Оба вентилятора на полных оборотах — громко, но ноутбук холоднее. "
                                          + "После перезагрузки снова включится автоматика."
                                        : "Для долгой тяжёлой нагрузки: игры, сборка, рендер. "
                                          + "Обычно автоматика справляется сама."
                                }
                            }
                        }

                        Label {
                            Layout.fillWidth: true
                            wrapMode: Text.Wrap
                            opacity: 0.6
                            font.pixelSize: 12
                            visible: Object.keys(win.modeLimits).length > 0
                            text: "Лимиты — замеры на WUJIE 15X от сети при полной загрузке всех ядер. "
                                + "От батареи и от блока питания слабее 100 Вт прошивка может ограничивать сильнее."
                        }
                        Label {
                            Layout.fillWidth: true
                            wrapMode: Text.Wrap
                            opacity: 0.6
                            font.pixelSize: 12
                            text: "Командой: openrevo perf office|balanced|turbo, openrevo fanmax on|off — можно назначить на сочетание клавиш."
                        }
                        Item { Layout.fillHeight: true }
                    }
                    }

                    // --- Батарея ---
                    ScrollView {
                        id: batScroll
                        clip: true
                        contentWidth: availableWidth

                    ColumnLayout {
                        id: batPage
                        width: batScroll.availableWidth
                        spacing: 14

                        function hm(hours) {
                            if (!isFinite(hours) || hours <= 0 || hours > 48)
                                return "";
                            const h = Math.floor(hours), m = Math.round((hours - h) * 60);
                            return h > 0 ? h + " ч " + m + " мин" : m + " мин";
                        }

                        Label {
                            text: "Батарея"
                            font.pixelSize: 16
                            font.bold: true
                        }

                        // Карточка состояния.
                        Rectangle {
                            Layout.fillWidth: true
                            implicitHeight: card.implicitHeight + 32
                            radius: 12
                            color: "#1f1f1f"

                            ColumnLayout {
                                id: card
                                anchors.fill: parent
                                anchors.margins: 16
                                spacing: 10

                                RowLayout {
                                    spacing: 16

                                    Label {
                                        text: ctl.batteryCapacity >= 0 ? ctl.batteryCapacity + "%" : "—"
                                        color: "white"
                                        font.pixelSize: 40
                                        font.bold: true
                                    }
                                    ColumnLayout {
                                        spacing: 2
                                        Label {
                                            color: "white"
                                            font.pixelSize: 15
                                            text: {
                                                const st = { "Charging": "⚡ Заряжается", "Discharging": "Разряжается",
                                                             "Not charging": "⏸ Не заряжается", "Full": "Заряжена" };
                                                // Зарядку остановил лимит, а не что-то непонятное.
                                                if (ctl.batteryStatus === "Not charging" && ctl.acOnline === 1
                                                        && win.chargeUpTo < 100 && ctl.batteryCapacity >= win.chargeUpTo - 2)
                                                    return "⏸ Заряд держится на уровне " + win.chargeUpTo + "%";
                                                return st[ctl.batteryStatus] || ctl.batteryStatus || "—";
                                            }
                                        }
                                        Label {
                                            color: "#bbbbbb"
                                            text: {
                                                const p = ctl.batteryPower;
                                                let s = ctl.acOnline === 1 ? "от сети" : "от батареи";
                                                if (Math.abs(p) >= 0.1)
                                                    s += " · " + (p > 0 ? "+" : "−") + Math.abs(p).toFixed(1) + " Вт";
                                                const e = ctl.batteryEnergy, f = ctl.batteryEnergyFull;
                                                if (p < -0.5 && e > 0)
                                                    s += " · осталось ~" + batPage.hm(e / -p);
                                                else if (p > 0.5 && e > 0 && f > 0) {
                                                    const t = batPage.hm((f - e) / p);
                                                    if (t) s += " · до полной ~" + t;
                                                }
                                                return s;
                                            }
                                        }
                                    }
                                }

                                // Полоса заряда.
                                Item {
                                    Layout.fillWidth: true
                                    implicitHeight: 26

                                    Rectangle {
                                        id: bar
                                        width: parent.width - 8
                                        height: 26
                                        radius: 6
                                        color: "#333333"
                                        clip: true

                                        Rectangle {
                                            width: parent.width * Math.max(0, ctl.batteryCapacity) / 100
                                            height: parent.height
                                            radius: 6
                                            color: ctl.batteryStatus === "Charging" ? "#ff6900"
                                                 : ctl.batteryCapacity <= 15 ? "#d9534f" : "#3cb371"
                                            Behavior on width { NumberAnimation { duration: 300 } }
                                        }
                                    }
                                    // Носик батареи.
                                    Rectangle {
                                        x: bar.width + 2; y: 7
                                        width: 5; height: 12; radius: 2
                                        color: "#333333"
                                    }
                                }

                                Label {
                                    color: "#999999"
                                    font.pixelSize: 12
                                    visible: text.length > 0
                                    text: {
                                        let s = [];
                                        const e = ctl.batteryEnergy, f = ctl.batteryEnergyFull, d = ctl.batteryEnergyDesign;
                                        if (e > 0 && f > 0)
                                            s.push(e.toFixed(1) + " из " + f.toFixed(1) + " Вт·ч");
                                        if (f > 0 && d > 0)
                                            s.push("полная ёмкость " + Math.round(f / d * 100) + "% от паспортной (" + d.toFixed(0) + " Вт·ч)");
                                        return s.join(" · ");
                                    }
                                }
                            }
                        }

                        Label {
                            text: "Профиль заряда"
                            font.pixelSize: 15
                            font.bold: true
                        }

                        RowLayout {
                            Layout.fillWidth: true
                            spacing: 10

                            Repeater {
                                model: ["high_capacity", "balanced", "stationary"]

                                ModeTile {
                                    required property string modelData
                                    readonly property string profile: modelData
                                    Layout.fillWidth: true
                                    Layout.preferredWidth: 1
                                    imageSize: 64
                                    image: "img/charge_" + profile + ".svg"
                                    caption: win.chargeTitle[profile]
                                    subtitle: win.hasChargeLimit ? "до " + win.chargeLimitOf[profile] + "%" : ""
                                    accent: win.chargeColor[profile]
                                    checked: ctl.chargeProfile === profile
                                    enabled: ctl.connected && (ctl.chargeProfile !== "" || win.hasChargeLimit)
                                    ToolTip.visible: hovered
                                    ToolTip.delay: 400
                                    ToolTip.text: win.chargeHint[profile]
                                    onClicked: {
                                        checked = Qt.binding(() => ctl.chargeProfile === profile);
                                        ctl.setCharge(profile);
                                    }
                                }
                            }
                        }

                        Label {
                            Layout.fillWidth: true
                            visible: win.hasChargeLimit && ctl.connected && ctl.chargeProfile === ""
                            wrapMode: Text.Wrap
                            text: "Сейчас задан свой лимит: заряд до " + win.chargeUpTo + "% (командой openrevo limit). "
                                + "Выберите профиль, чтобы вернуться к обычным настройкам."
                        }

                        Label {
                            Layout.fillWidth: true
                            wrapMode: Text.Wrap
                            opacity: 0.7
                            visible: win.hasChargeLimit
                            text: "Батарея стареет быстрее, если подолгу держать её заряженной до 100%. "
                                + "Если ноутбук в основном работает от розетки, выберите «Баланс» или «Всегда от сети»: "
                                + "зарядка остановится на нужном уровне, а ноутбук продолжит работать от сети. "
                                + "Перед поездкой включите «Полный заряд».\n\n"
                                + "Если заряд уже выше выбранного уровня, он не сбрасывается — батарея просто "
                                + "не заряжается, пока не разрядится ниже. Настройка хранится в самом ноутбуке "
                                + "и работает, даже когда программа закрыта."
                        }

                        Label {
                            Layout.fillWidth: true
                            wrapMode: Text.Wrap
                            opacity: 0.7
                            visible: !win.hasChargeLimit
                            text: "Профиль хранит контроллер ноутбука (EC). В щадящих профилях прошивка уменьшает "
                                + "ёмкость, которую батарея считает полной: индикатор всё равно доходит до 100%, "
                                + "а реальный запас меньше — смотрите Вт·ч выше."
                        }
                        Item { Layout.fillHeight: true }
                    }
                    }

                    // --- Клавиатура ---
                    ColumnLayout {
                        spacing: 12

                        Label {
                            text: "Клавиатура"
                            font.pixelSize: 16
                            font.bold: true
                        }

                        // Подсветка.
                        RowLayout {
                            Layout.fillWidth: true
                            visible: ctl.kbdBrightness >= 0
                            spacing: 10

                            Switch {
                                enabled: ctl.connected
                                checked: ctl.kbdBrightness > 0
                                onToggled: {
                                    const want = checked;
                                    checked = Qt.binding(() => ctl.kbdBrightness > 0);
                                    ctl.requestKbdBrightness(want ? (win.kbdOnLevel || Math.ceil(ctl.kbdMax / 2)) : 0);
                                }
                            }
                            ColumnLayout {
                                Layout.fillWidth: true
                                spacing: 0
                                Label {
                                    text: "Подсветка клавиатуры"
                                    font.bold: true
                                }
                                Label {
                                    Layout.fillWidth: true
                                    wrapMode: Text.Wrap
                                    opacity: 0.65
                                    font.pixelSize: 12
                                    text: "Яркость меняется и клавишей Fn на ноутбуке — программа её запомнит "
                                        + "и вернёт после перезагрузки."
                                }
                            }
                        }

                        RowLayout {
                            Layout.fillWidth: true
                            Layout.leftMargin: 4
                            visible: ctl.kbdBrightness >= 0 && ctl.kbdMax > 1
                            spacing: 6

                            Label {
                                text: "Яркость"
                                Layout.preferredWidth: 70
                            }
                            Repeater {
                                model: ctl.kbdMax
                                Button {
                                    required property int index
                                    readonly property int level: index + 1
                                    Layout.preferredWidth: 44
                                    text: level
                                    checkable: true
                                    checked: ctl.kbdBrightness === level
                                    enabled: ctl.connected
                                    onClicked: {
                                        checked = Qt.binding(() => ctl.kbdBrightness === level);
                                        ctl.requestKbdBrightness(level);
                                    }
                                }
                            }
                            Item { Layout.fillWidth: true }
                        }

                        RowLayout {
                            Layout.fillWidth: true
                            Layout.leftMargin: 4
                            visible: ctl.kbdBrightness >= 0 && ctl.kbdColor !== ""
                            spacing: 8

                            Label {
                                text: "Цвет"
                                Layout.preferredWidth: 70
                            }
                            Repeater {
                                model: win.kbdColors
                                Rectangle {
                                    id: swatch
                                    required property var modelData
                                    readonly property bool current: Qt.colorEqual(ctl.kbdColor, modelData.c)
                                    width: 28
                                    height: 28
                                    radius: 14
                                    color: modelData.c
                                    border.width: current ? 3 : 1
                                    border.color: current ? "#2f7fc1" : Qt.darker(win.palette.window, 1.4)
                                    opacity: ctl.connected ? 1 : 0.4

                                    MouseArea {
                                        id: swatchArea
                                        anchors.fill: parent
                                        hoverEnabled: true
                                        cursorShape: Qt.PointingHandCursor
                                        enabled: ctl.connected
                                        onClicked: ctl.requestKbdColor(swatch.modelData.c)
                                    }
                                    ToolTip.visible: swatchArea.containsMouse
                                    ToolTip.delay: 400
                                    ToolTip.text: swatch.modelData.name
                                }
                            }
                            Item { Layout.fillWidth: true }
                        }

                        Item { implicitHeight: 4; visible: ctl.kbdBrightness >= 0 }

                        RowLayout {
                            Layout.fillWidth: true
                            spacing: 10

                            Switch {
                                enabled: ctl.connected && ctl.fnLock >= 0
                                checked: ctl.fnLock === 1
                                onToggled: {
                                    const want = checked;
                                    checked = Qt.binding(() => ctl.fnLock === 1);
                                    ctl.requestFnLock(want);
                                }
                            }
                            ColumnLayout {
                                Layout.fillWidth: true
                                spacing: 0
                                Label {
                                    text: ctl.fnLock < 0 ? "Fn Lock не поддерживается" : "Fn Lock"
                                    font.bold: true
                                }
                                Label {
                                    Layout.fillWidth: true
                                    wrapMode: Text.Wrap
                                    opacity: 0.65
                                    font.pixelSize: 12
                                    text: ctl.fnLock === 1
                                        ? "Верхний ряд работает как F1–F12, мультимедиа — с Fn."
                                        : "Верхний ряд — мультимедиа и яркость, F1–F12 — с Fn."
                                }
                            }
                        }

                        Label {
                            Layout.fillWidth: true
                            wrapMode: Text.Wrap
                            opacity: 0.7
                            text: "Командой: <tt>openrevo fnlock toggle</tt>, <tt>openrevo kbd toggle</tt>, "
                                + "<tt>openrevo kbd color blue</tt>."
                        }
                        Item { Layout.fillHeight: true }
                    }

                    // --- Система ---
                    ColumnLayout {
                        spacing: 8

                        Label {
                            text: "Питание"
                            font.pixelSize: 16
                            font.bold: true
                            visible: ctl.usbPowershare >= 0 || ctl.acAutoBoot >= 0
                        }

                        Repeater {
                            model: [
                                { key: "usb", title: "USB при выключенном ноутбуке",
                                  hint: "Порты USB дают питание, когда ноутбук выключен, — можно заряжать телефон. "
                                      + "От батареи это расходует заряд." },
                                { key: "boot", title: "Включаться при подключении зарядки",
                                  hint: "Ноутбук сам включится, как только к нему подключат зарядку." }
                            ]
                            RowLayout {
                                id: powerRow
                                required property var modelData
                                readonly property int value: modelData.key === "usb" ? ctl.usbPowershare : ctl.acAutoBoot
                                Layout.fillWidth: true
                                visible: value >= 0
                                spacing: 10

                                Switch {
                                    enabled: ctl.connected
                                    checked: powerRow.value === 1
                                    onToggled: {
                                        const want = checked;
                                        checked = Qt.binding(() => powerRow.value === 1);
                                        if (powerRow.modelData.key === "usb")
                                            ctl.requestUsbPowershare(want);
                                        else
                                            ctl.requestAcAutoBoot(want);
                                    }
                                }
                                ColumnLayout {
                                    Layout.fillWidth: true
                                    spacing: 0
                                    Label {
                                        text: powerRow.modelData.title
                                        font.bold: true
                                    }
                                    Label {
                                        Layout.fillWidth: true
                                        wrapMode: Text.Wrap
                                        opacity: 0.65
                                        font.pixelSize: 12
                                        text: powerRow.modelData.hint
                                    }
                                }
                            }
                        }

                        Item { implicitHeight: 8; visible: ctl.usbPowershare >= 0 || ctl.acAutoBoot >= 0 }

                        Label {
                            text: "Сведения о системе"
                            font.pixelSize: 16
                            font.bold: true
                        }
                        TextArea {
                            Layout.fillWidth: true
                            readOnly: true
                            selectByMouse: true
                            wrapMode: Text.Wrap
                            text: ctl.deviceInfo + (ctl.connected ? "\nБэкенд: " + ctl.backend : "")
                        }
                        Item { Layout.fillHeight: true }
                    }

                    // --- О программе ---
                    ScrollView {
                        id: aboutView
                        clip: true
                        contentWidth: availableWidth

                        Label {
                            width: aboutView.availableWidth
                            wrapMode: Text.Wrap
                            textFormat: Text.MarkdownText
                            onLinkActivated: link => Qt.openUrlExternally(link)
                            text: "## Open Revo для Linux\n\n"
                                + "Режимы производительности и профили заряда ноутбуков MECHREVO на платформе "
                                + "Uniwill / TongFang — то, что в Windows делает фирменный ControlCenter.\n\n"
                                + "Работает через [tuxedo-drivers](https://gitlab.com/tuxedocomputers/development/packages/tuxedo-drivers) "
                                + "(© TUXEDO Computers) с патчами для MECHREVO; патч проверки производителя — из пакета "
                                + "[mechrevo-drivers-dkms](https://aur.archlinux.org/packages/mechrevo-drivers-dkms). "
                                + "Значения режимов сверены с [OpenRevo](https://github.com/faintonce/open-revo) для Windows.\n\n"
                                + "**Внимание:** авторы не связаны с MECHREVO, TongFang и TUXEDO и не несут ответственности "
                                + "за возможные неполадки. Используйте на свой страх и риск.\n\n"
                                + "Лицензия: GNU GPL v3 или новее; драйвер — GNU GPL v2."
                        }
                    }
                }
            }
        }
    }
}
