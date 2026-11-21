# Libraries the settings app (egui/winit/glow) loads at run time.
pkgs: with pkgs; [
  libGL
  libxkbcommon
  wayland
  libx11
  libxcursor
  libxi
  libxrandr
]
