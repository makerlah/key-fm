"""Small shared constants."""
import os
import sys

APP = "keyfm"
IS_WIN = sys.platform.startswith("win")
ALT_MASK = 0x20000 if IS_WIN else 0x8

THIS_PC = "::this-pc::"
RECYCLE = "::recycle-bin::"
VIRTUAL = (THIS_PC, RECYCLE)

COLS = ("name", "where", "size", "modified")
TITLES = {"name": "Name", "where": "Original location", "size": "Size",
          "modified": "Modified"}
PC_TITLES = {"name": "Name", "where": "", "size": "Free space", "modified": ""}
BIN_TITLES = {"name": "Name", "where": "Original location", "size": "Size",
              "modified": "Date deleted"}

CONFIG_FILE = os.path.join(os.path.expanduser("~"), ".keyfm.json")
OLD_PIN_FILE = os.path.join(os.path.expanduser("~"), ".keyfm_pins.json")

QUICK_ICONS = {"Home": "🏠", "Desktop": "🖥", "Downloads": "📥",
               "Documents": "🗂", "Pictures": "🖼", "Music": "🎵",
               "Videos": "🎬", "Recycle Bin": "🗑"}