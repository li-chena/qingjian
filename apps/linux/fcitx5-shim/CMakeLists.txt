cmake_minimum_required(VERSION 3.16)
project(qingjian-fcitx5 CXX)

set(CMAKE_CXX_STANDARD 20)   # fcitx5 5.1.x 头文件用 std::ranges/std::span,20 起步
set(CMAKE_CXX_STANDARD_REQUIRED ON)

find_package(Fcitx5Core REQUIRED)

# Rust 侧 staticlib:先 cargo build -p qingjian-linux-host --release,
# 路径可用 -DQINGJIAN_RUST_LIB=... 覆盖(默认取仓库 target/release)。
if(NOT DEFINED QINGJIAN_RUST_LIB)
    set(QINGJIAN_RUST_LIB
        "${CMAKE_CURRENT_SOURCE_DIR}/../../../target/release/libqingjian_linux_host.a")
endif()
if(NOT EXISTS "${QINGJIAN_RUST_LIB}")
    message(FATAL_ERROR "Rust staticlib 不在:${QINGJIAN_RUST_LIB}(先 cargo build -p qingjian-linux-host --release)")
endif()

# 产物 libqingjian.so(保留 lib 前缀,与本机 libpinyin.so 等现役插件同惯例)
add_library(qingjian MODULE addon.cpp)
target_link_libraries(qingjian
    Fcitx5::Core
    "${QINGJIAN_RUST_LIB}"
    pthread dl m)
