# cliprs
cliprs is a lightweight, rust-native replay-buffer clipper, built for anyone who wants their hardware to themselves.
It reads the KMS primary plane directly, not needing any compositor-specific portal workarounds. It works the same on any Wayland, X11 or TTY display as long as you are getting monitor output.
The tool uses very low RAM (~220MB at 1440p 60fps with a 30sec buffer) because it encodes the captured frames to H.264 directly on the GPU using Vulkan, with no uncompressed frame touching the CPU.
> [!NOTE]
> This project is still in very, very early development and not in a usable state yet.

## Requirements
This tool exclusively works on Linux, although Windows support may be coming in the (not so near) future.
You also need a GPU that supports Vulkan Video H.264 encode, which should be present for most modern cards. Only tested on an AMD Radeon 9060XT.
To capture the KMS framebuffer, the daemon has to run as root. It only communicates with the GUI via config files and the clip files themselves (including metadata), to keep attack surface as small as possible.

## Limitations
- Some monitor color formats still fail to parse, but the common ones should work
- A resolution change mid-clip currently crashes the daemon, as it expects the format of data to stay consistent
- Audio is not yet supported
- GPU device enumeration is not yet added, so the DRM device ID is hardcoded to /dev/dri/card2

## License
This project is licensed under GPLv3.
