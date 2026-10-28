Open Revo — переносная сборка
==============================

Режимы производительности и профиль заряда ноутбуков MECHREVO (Uniwill /
TongFang) без установки в систему. Qt 6 встроен.

1. Собрать драйвер под своё ядро (один раз после каждого обновления ядра).
   Нужны заголовки ядра и компилятор:
     Fedora:  sudo dnf install kernel-devel gcc make
     Ubuntu:  sudo apt install linux-headers-$(uname -r) build-essential

     ./build-driver.sh

2. Проверить, что драйвер видит ноутбук (только чтение):

     ./run.sh probe

   «режимов в драйвере: 3» — режимы работают. 0 — плата не в списке драйвера:
   профиль заряда и Fn Lock будут работать, режимы — нет (см. README проекта,
   раздел «Проверка своей модели»).

3. Запуск: демон от root (sudo) + окно; выход — через меню в трее.

     ./run.sh

   Без ноутбука MECHREVO — эмуляция, root не нужен:

     ./run.sh --mock

С включённым Secure Boot самосборные модули не загрузятся («Key was rejected
by service»): поставьте пакет (.rpm/.deb — DKMS подпишет модули) или
подпишите их своим ключом MOK.

Если в системе уже стоит драйвер tuxedo-drivers без патчей для MECHREVO,
удалите его: run.sh загрузит его вместо собранного здесь.

Лог демона: state/daemon.log. Сайт: https://github.com/AbyssMoon/open-revo-linux
