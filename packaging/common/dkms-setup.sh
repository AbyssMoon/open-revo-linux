#!/bin/sh
# Общая часть установки пакета (RPM %post и deb postinst):
#   dkms-setup.sh install ВЕРСИЯ  — собрать драйвер под все ядра с заголовками,
#                                   убрать старые версии, перезагрузить модули;
#   dkms-setup.sh remove ВЕРСИЯ   — выгрузить модули и удалить драйвер из DKMS.
# Лежит в /opt/openrevo/libexec, чтобы спек и deb не расходились.
NAME=openrevo-drivers
ACTION="$1"
VER="$2"
MODULES="tuxedo_io uniwill_wmi clevo_wmi clevo_acpi tuxedo_keyboard tuxedo_compatibility_check"

unload() {
    for m in $MODULES; do
        modprobe -r "$m" 2>/dev/null || true
    done
}

case "$ACTION" in
    install)
        # Старые версии драйвера: обновление пакета оставило бы их в DKMS.
        for v in $(dkms status -m "$NAME" 2>/dev/null | sed -n "s|^$NAME/\([^,:]*\)[,:].*|\1|p" | sort -u); do
            [ "$v" = "$VER" ] || dkms remove -m "$NAME" -v "$v" --all >/dev/null 2>&1 || true
        done
        dkms status -m "$NAME" -v "$VER" | grep -q . || dkms add -m "$NAME" -v "$VER" || true
        # Под все ядра, для которых есть заголовки (в том числе установленное,
        # но ещё не загруженное); новые ядра DKMS подхватит сам.
        for k in /lib/modules/*; do
            [ -e "$k/build" ] || continue
            dkms install -m "$NAME" -v "$VER" -k "$(basename "$k")" >/dev/null 2>&1 || \
                echo "openrevo: не удалось собрать драйвер для ядра $(basename "$k")" >&2
        done
        # Загруженные модули могли остаться от прошлой версии. Демон держит
        # /dev/tuxedo_io открытым, и tuxedo_io не выгрузится — останавливаем
        # демон на это время. uniwill_wmi при загрузке системы подтягивает udev
        # по WMI-устройству, а после выгрузки — нет, поэтому грузим его явно.
        daemon_was_active=
        if systemctl is-active -q openrevod.service 2>/dev/null; then
            daemon_was_active=1
            systemctl stop openrevod.service || true
        fi
        unload
        for m in tuxedo_keyboard uniwill_wmi tuxedo_io; do
            modprobe "$m" 2>/dev/null || true
        done
        [ -z "$daemon_was_active" ] || systemctl start openrevod.service || true
        ;;
    remove)
        unload
        dkms remove -m "$NAME" -v "$VER" --all >/dev/null 2>&1 || true
        ;;
    *)
        echo "использование: $0 install|remove ВЕРСИЯ" >&2
        exit 2
        ;;
esac
