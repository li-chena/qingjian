#!/usr/bin/env bash
# 青简 Linux 端用户级安装:不动系统目录,全部落用户目录。
# 用法:bash apps/linux/install.sh   (装完自动重启 fcitx5)
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
so="$repo/build/fcitx5-shim/libqingjian.so"
[[ -f $so ]] || { echo "先构建:cargo build -p qingjian-linux-host --release && cmake -S apps/linux/fcitx5-shim -B build/fcitx5-shim && cmake --build build/fcitx5-shim" >&2; exit 1; }

lib_dir="$HOME/.local/lib/fcitx5"
data_dir="${XDG_DATA_HOME:-$HOME/.local/share}/fcitx5"
install -Dm755 "$so" "$lib_dir/libqingjian.so"
install -Dm644 "$repo/apps/linux/fcitx5-shim/conf/addon-qingjian.conf" "$data_dir/addon/qingjian.conf"
install -Dm644 "$repo/apps/linux/fcitx5-shim/conf/inputmethod-qingjian.conf" "$data_dir/inputmethod/qingjian.conf"

# 数据文件:全量词库 + 英语释义表(user.tsv 学习数据不碰)。
qdata="${XDG_DATA_HOME:-$HOME/.local/share}/qingjian"
install -Dm644 "$repo/assets/lexicon/dict.tsv" "$qdata/dict.tsv"
install -Dm644 "$repo/assets/glossary/glossary-en.tsv" "$qdata/glossary-en.tsv"

# .so 的搜索路径要靠 FCITX_ADDON_DIRS(conf 文件用户目录原生支持,不用它)。
# 写进 environment.d 供下次登录;本次立即生效靠下面带环境变量重启。
env_file="$HOME/.config/environment.d/qingjian-fcitx5.conf"
mkdir -p "$(dirname "$env_file")"
echo "FCITX_ADDON_DIRS=$lib_dir:/usr/lib/fcitx5" > "$env_file"

echo "已安装:$lib_dir/libqingjian.so + addon/inputmethod conf + 词库数据"
# 延迟重启 + setsid 脱离终端:立即替换 fcitx5 会吞掉启动本脚本那次回车的松键事件,
# Wayland 合成器会当成回车一直按着 → 终端被无限回车(0915 实锤)。
# sleep 给松键留窗口;setsid 让新 fcitx5 不当终端的子进程。
setsid bash -c "sleep 0.5; FCITX_ADDON_DIRS='$lib_dir:/usr/lib/fcitx5' exec fcitx5 -rd" >/dev/null 2>&1 </dev/null &
echo "fcitx5 将在半秒后带新插件重启;在输入法配置里添加「青简」即可(fcitx5-configtool)。"
