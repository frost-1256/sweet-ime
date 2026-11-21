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

# App icon for the settings app's desktop entry. The input method itself
# keeps the theme's keyboard icon.
set(SWEET_ICON_DIR "${CMAKE_CURRENT_SOURCE_DIR}/../../assets/icon")
foreach(size 16 24 32 48 64 128 256 512)
    install(FILES "${SWEET_ICON_DIR}/sweet-ime-${size}.png"
        DESTINATION "${CMAKE_INSTALL_DATADIR}/icons/hicolor/${size}x${size}/apps"
        RENAME "sweet-ime.png")
endforeach()
