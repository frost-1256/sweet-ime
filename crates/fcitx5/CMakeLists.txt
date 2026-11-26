# fcitx5 addon: the Rust engine (static library built with cargo) plus a
# small C++ frontend.
cmake_minimum_required(VERSION 3.16)
project(sweet-ime-fcitx5 VERSION 0.1.2 LANGUAGES C CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_VISIBILITY_PRESET hidden)

include(GNUInstallDirs)
find_package(Fcitx5Core 5.0.0 REQUIRED)
find_package(Threads REQUIRED)

set(SWEET_DATADIR "${CMAKE_INSTALL_FULL_DATADIR}/sweet-ime" CACHE PATH "Where system.dict is installed")
set(SWEET_SETTINGS "${CMAKE_INSTALL_FULL_BINDIR}/sweet-settings" CACHE FILEPATH "Settings app")

if(DEFINED ENV{CARGO_TARGET_DIR})
    set(CARGO_TARGET_DIR "$ENV{CARGO_TARGET_DIR}")
else()
    set(CARGO_TARGET_DIR "${CMAKE_BINARY_DIR}/cargo")
endif()
set(SWEET_RUST_LIB "${CARGO_TARGET_DIR}/release/libsweet_fcitx5.a")

add_custom_target(sweet_rust
    COMMAND ${CMAKE_COMMAND} -E env
        "CARGO_TARGET_DIR=${CARGO_TARGET_DIR}"
        "SWEET_IME_DATADIR=${SWEET_DATADIR}"
        cargo build --release --locked -p sweet-fcitx5
    WORKING_DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}"
    BYPRODUCTS "${SWEET_RUST_LIB}"
    USES_TERMINAL
    COMMENT "Building the Rust engine")

add_library(sweetime MODULE cpp/engine.cpp)
add_dependencies(sweetime sweet_rust)
set_target_properties(sweetime PROPERTIES PREFIX "")
target_compile_definitions(sweetime PRIVATE "SWEET_SETTINGS_PATH=\"${SWEET_SETTINGS}\"")
target_link_libraries(sweetime PRIVATE Fcitx5::Core "${SWEET_RUST_LIB}" Threads::Threads ${CMAKE_DL_LIBS} m)
# Keep the Rust symbols private to the module.
target_link_options(sweetime PRIVATE "LINKER:--exclude-libs,ALL" "LINKER:--as-needed")

configure_file(data/sweetime-addon.conf.in "${CMAKE_CURRENT_BINARY_DIR}/sweetime-addon.conf" @ONLY)
install(TARGETS sweetime DESTINATION "${CMAKE_INSTALL_LIBDIR}/fcitx5")
install(FILES "${CMAKE_CURRENT_BINARY_DIR}/sweetime-addon.conf"
    DESTINATION "${CMAKE_INSTALL_DATADIR}/fcitx5/addon" RENAME sweetime.conf)
install(FILES data/sweetime.conf DESTINATION "${CMAKE_INSTALL_DATADIR}/fcitx5/inputmethod")

# Custom animated UserInterface addon (sweetui): Cairo + Pango renderer with
# Wayland (wlr-layer-shell-v1) and X11/XCB backends.
find_package(PkgConfig QUIET)
if(PkgConfig_FOUND)
    pkg_check_modules(CAIRO IMPORTED_TARGET cairo)
    pkg_check_modules(PANGOCAIRO IMPORTED_TARGET pangocairo)
    pkg_check_modules(WAYLAND_CLIENT IMPORTED_TARGET wayland-client)
    pkg_check_modules(XCB IMPORTED_TARGET xcb)
    pkg_check_modules(XCB_EWMH IMPORTED_TARGET xcb-ewmh)
endif()
find_package(Fcitx5ModuleWayland QUIET)
find_package(Fcitx5ModuleXCB QUIET)
find_package(Fcitx5ModuleNotificationItem QUIET)
find_program(WAYLAND_SCANNER wayland-scanner)

if(CAIRO_FOUND AND PANGOCAIRO_FOUND)
    set(SWEETUI_SRCS cpp/ui.cpp)
    set(SWEETUI_LIBS Fcitx5::Core "${SWEET_RUST_LIB}" PkgConfig::CAIRO PkgConfig::PANGOCAIRO Threads::Threads ${CMAKE_DL_LIBS} m)
    set(SWEETUI_DEFS)

    if(WAYLAND_CLIENT_FOUND AND Fcitx5ModuleWayland_FOUND AND WAYLAND_SCANNER)
        set(XDG_XML "${CMAKE_CURRENT_SOURCE_DIR}/protocols/xdg-shell.xml")
        set(LAYER_XML "${CMAKE_CURRENT_SOURCE_DIR}/protocols/wlr-layer-shell-unstable-v1.xml")
        set(BGFX_XML "${CMAKE_CURRENT_SOURCE_DIR}/protocols/ext-background-effect-v1.xml")
        set(IMV2_XML "${CMAKE_CURRENT_SOURCE_DIR}/protocols/input-method-unstable-v2.xml")
        set(XDG_H "${CMAKE_CURRENT_BINARY_DIR}/xdg-shell-client-protocol.h")
        set(XDG_C "${CMAKE_CURRENT_BINARY_DIR}/xdg-shell-client-protocol.c")
        set(LAYER_H "${CMAKE_CURRENT_BINARY_DIR}/wlr-layer-shell-unstable-v1-client-protocol.h")
        set(LAYER_C "${CMAKE_CURRENT_BINARY_DIR}/wlr-layer-shell-unstable-v1-client-protocol.c")
        set(BGFX_H "${CMAKE_CURRENT_BINARY_DIR}/ext-background-effect-v1-client-protocol.h")
        set(BGFX_C "${CMAKE_CURRENT_BINARY_DIR}/ext-background-effect-v1-client-protocol.c")
        set(IMV2_H "${CMAKE_CURRENT_BINARY_DIR}/input-method-unstable-v2-client-protocol.h")
        set(IMV2_C "${CMAKE_CURRENT_BINARY_DIR}/input-method-unstable-v2-client-protocol.c")
        add_custom_command(OUTPUT "${XDG_H}"
            COMMAND "${WAYLAND_SCANNER}" client-header "${XDG_XML}" "${XDG_H}"
            DEPENDS "${XDG_XML}")
        add_custom_command(OUTPUT "${XDG_C}"
            COMMAND "${WAYLAND_SCANNER}" private-code "${XDG_XML}" "${XDG_C}"
            DEPENDS "${XDG_XML}")
        add_custom_command(OUTPUT "${LAYER_H}"
            COMMAND "${WAYLAND_SCANNER}" client-header "${LAYER_XML}" "${LAYER_H}"
            DEPENDS "${LAYER_XML}")
        add_custom_command(OUTPUT "${LAYER_C}"
            COMMAND "${WAYLAND_SCANNER}" private-code "${LAYER_XML}" "${LAYER_C}"
            DEPENDS "${LAYER_XML}")
        add_custom_command(OUTPUT "${BGFX_H}"
            COMMAND "${WAYLAND_SCANNER}" client-header "${BGFX_XML}" "${BGFX_H}"
            DEPENDS "${BGFX_XML}")
        add_custom_command(OUTPUT "${BGFX_C}"
            COMMAND "${WAYLAND_SCANNER}" private-code "${BGFX_XML}" "${BGFX_C}"
            DEPENDS "${BGFX_XML}")
        add_custom_command(OUTPUT "${IMV2_H}"
            COMMAND "${WAYLAND_SCANNER}" client-header "${IMV2_XML}" "${IMV2_H}"
            DEPENDS "${IMV2_XML}")
        add_custom_command(OUTPUT "${IMV2_C}"
            COMMAND "${WAYLAND_SCANNER}" private-code "${IMV2_XML}" "${IMV2_C}"
            DEPENDS "${IMV2_XML}")
        list(APPEND SWEETUI_SRCS "${XDG_H}" "${XDG_C}" "${LAYER_H}" "${LAYER_C}" "${BGFX_H}" "${BGFX_C}" "${IMV2_H}" "${IMV2_C}")
        list(APPEND SWEETUI_LIBS Fcitx5::Module::Wayland PkgConfig::WAYLAND_CLIENT)
        list(APPEND SWEETUI_DEFS SWEET_HAVE_WAYLAND=1)
    endif()

    if(XCB_FOUND AND XCB_EWMH_FOUND AND Fcitx5ModuleXCB_FOUND)
        list(APPEND SWEETUI_LIBS Fcitx5::Module::XCB PkgConfig::XCB PkgConfig::XCB_EWMH)
        list(APPEND SWEETUI_DEFS SWEET_HAVE_XCB=1)
    endif()

    if(Fcitx5ModuleNotificationItem_FOUND)
        list(APPEND SWEETUI_LIBS Fcitx5::Module::NotificationItem)
        list(APPEND SWEETUI_DEFS SWEET_HAVE_NOTIFICATIONITEM=1)
    endif()

    if(SWEETUI_DEFS)
        add_library(sweetui MODULE ${SWEETUI_SRCS})
        add_dependencies(sweetui sweet_rust)
        set_target_properties(sweetui PROPERTIES PREFIX "")
        target_include_directories(sweetui PRIVATE "${CMAKE_CURRENT_BINARY_DIR}")
        target_compile_definitions(sweetui PRIVATE ${SWEETUI_DEFS})
        target_link_libraries(sweetui PRIVATE ${SWEETUI_LIBS})
        target_link_options(sweetui PRIVATE "LINKER:--exclude-libs,ALL" "LINKER:--as-needed")
        configure_file(data/sweetui-addon.conf.in "${CMAKE_CURRENT_BINARY_DIR}/sweetui-addon.conf" @ONLY)
        install(TARGETS sweetui DESTINATION "${CMAKE_INSTALL_LIBDIR}/fcitx5")
        install(FILES "${CMAKE_CURRENT_BINARY_DIR}/sweetui-addon.conf"
            DESTINATION "${CMAKE_INSTALL_DATADIR}/fcitx5/addon" RENAME sweetui.conf)
    endif()
endif()

# App icon for the settings app's desktop entry. The input method itself
# keeps the theme's keyboard icon.
set(SWEET_ICON_DIR "${CMAKE_CURRENT_SOURCE_DIR}/../../assets/icon")
foreach(size 16 24 32 48 64 128 256 512)
    install(FILES "${SWEET_ICON_DIR}/sweet-ime-${size}.png"
        DESTINATION "${CMAKE_INSTALL_DATADIR}/icons/hicolor/${size}x${size}/apps"
        RENAME "sweet-ime.png")
endforeach()
