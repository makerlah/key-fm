"""Entry point: `python -m keyfm [start_directory]`."""
import os
import sys
import tkinter as tk

from .ui import FileManager


def main():
    root = tk.Tk()
    start_dir = sys.argv[1] if len(sys.argv) > 1 else os.path.expanduser("~")
    FileManager(root, start_dir)
    root.mainloop()


if __name__ == "__main__":
    main()