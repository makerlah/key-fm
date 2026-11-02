"""Entry point: `python -m keyfm [start_directory]`."""
import os
import sys

from . import __version__


def main():
    # #26: --help / --version (handled before Tk is created)
    if len(sys.argv) > 1 and sys.argv[1] in ("-h", "--help"):
        print("usage: python -m keyfm [start_directory]")
        return
    if len(sys.argv) > 1 and sys.argv[1] in ("-V", "--version"):
        print(__version__)
        return
    # #27: validate the start directory
    start_dir = (os.path.expanduser(sys.argv[1])
                 if len(sys.argv) > 1 else os.path.expanduser("~"))
    if not os.path.isdir(start_dir):
        start_dir = os.path.expanduser("~")
    import tkinter as tk
    from .ui import FileManager
    root = tk.Tk()
    try:  # #25: always destroy the Tk root
        FileManager(root, start_dir)
        root.mainloop()
    finally:
        try:
            root.destroy()
        except tk.TclError:
            pass


if __name__ == "__main__":
    main()