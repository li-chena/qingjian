#!/usr/bin/env bash
# 青简 Linux 端用户级安装:不动系统目录,全部落用户目录。
# 双模式(安装逻辑单一真相源):
#   开发模式 = 在 git 仓库里跑(bash apps/linux/install.sh),从 build/ 与 assets/ 取。
#   分发模式 = 在解开的安装包里跑(bash install.sh),从脚本同目录的 libqingjian.so/conf/theme/data 取。
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if [[ -f "$here/libqingjian.so" ]]; then
    # 分发模式:payload 就在脚本旁。
    so="$here/libqingjian.so"
    src_conf="$here/conf"
    src_theme="$here/theme"
    src_data="$here/data"
else
    # 开发模式:回到仓库根。
    repo="$(cd "$here/../.." && pwd)"
    so="$repo/build/fcitx5-shim/libqingjian.so"
    [[ -f $so ]] || { echo "先构建:cargo build -p qingjian-linux-host --release && cmake -S apps/linux/fcitx5-shim -B build/fcitx5-shim && cmake --build build/fcitx5-shim" >&2; exit 1; }
    src_conf="$repo/apps/linux/fcitx5-shim/conf"
    src_theme="$repo/apps/linux/theme"
    src_data="$repo/assets"  # 开发模式数据分散在 assets/lexicon 与 assets/glossary,下面单独处理
fi

lib_dir="$HOME/.local/lib/fcitx5"
data_dir="${XDG_DATA_HOME:-$HOME/.local/share}/fcitx5"
install -Dm755 "$so" "$lib_dir/libqingjian.so"
install -Dm644 "$src_conf/addon-qingjian.conf" "$data_dir/addon/qingjian.conf"
install -Dm644 "$src_conf/inputmethod-qingjian.conf" "$data_dir/inputmethod/qingjian.conf"

# 数据文件:全量词库 + 各语种释义表 + 英文词表(user.tsv 学习数据不碰)。
qdata="${XDG_DATA_HOME:-$HOME/.local/share}/qingjian"
put_data() { # put_data <目标文件名> <开发模式源相对 assets 的路径>
    local name="$1" dev_rel="$2"
    if [[ -f "$src_data/$name" ]]; then
        install -Dm644 "$src_data/$name" "$qdata/$name"          # 分发模式:data/ 平铺
    else
        install -Dm644 "$src_data/$dev_rel" "$qdata/$name"       # 开发模式:assets 子目录
    fi
}
put_data dict.tsv        lexicon/dict.tsv
put_data glossary-en.tsv glossary/glossary-en.tsv
put_data english.tsv     lexicon/english.tsv
put_data glossary-zh.tsv glossary/glossary-zh.tsv
put_data glossary-ja.tsv glossary/glossary-ja.tsv
put_data glossary-es.tsv glossary/glossary-es.tsv

# 播种配置文件(已存在则不动):模糊音开常用四路,其余键列全供随手改;改完敲下个键即热生效。
qj_config="$HOME/.config/qingjian/config.toml"
if [[ ! -f $qj_config ]]; then
    mkdir -p "$(dirname "$qj_config")"
    cat > "$qj_config" <<'QJCONF'
# 青简配置。保存后敲下一个键即生效(热加载),不用重启。
[general]
learning_language = "en"   # 学习语言:en 英语 / ja 日语 / es 西班牙语
page_size = 9              # 每页候选数,1-9

[fuzzy]
z_zh = true
c_ch = true
s_sh = true
n_l = true
f_h = false
l_r = false
an_ang = false
en_eng = false
in_ing = false
QJCONF
fi

# 青简候选窗主题(亮/暗两套)+ classicui 配置:竖排、青简主题、跟随系统明暗。
theme_dir="$data_dir/themes"
for t in qingjian qingjian-dark; do
    install -Dm644 "$src_theme/$t/theme.conf"    "$theme_dir/$t/theme.conf"
    install -Dm644 "$src_theme/$t/panel.svg"     "$theme_dir/$t/panel.svg"
    install -Dm644 "$src_theme/$t/highlight.svg" "$theme_dir/$t/highlight.svg"
done

classicui_conf="$HOME/.config/fcitx5/conf/classicui.conf"
mkdir -p "$(dirname "$classicui_conf")"
touch "$classicui_conf"
set_conf() { # set_conf <key> <value>:有则替换,无则追加(classicui 配置是平铺 key=value)
    local key="$1" value="$2"
    if grep -q "^$key=" "$classicui_conf"; then
        sed -i "s|^$key=.*|$key=$value|" "$classicui_conf"
    else
        echo "$key=$value" >> "$classicui_conf"
    fi
}
set_conf "Theme" "qingjian"
set_conf "DarkTheme" "qingjian-dark"
set_conf "UseDarkTheme" "True"
set_conf "Vertical Candidate List" "True"
set_conf "Font" "Sans 12"

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
