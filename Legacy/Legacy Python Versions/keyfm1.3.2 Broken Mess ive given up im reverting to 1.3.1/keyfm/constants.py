"""Small shared constants."""
import os
import sys

APP = "keyfm"
IS_WIN = sys.platform.startswith("win")
ALT_MASK = 0x20000 if IS_WIN else 0x8

THIS_PC = "::this-pc::"
RECYCLE = "::recycle-bin::"
VIRTUAL = (THIS_PC, RECYCLE)

COLS = ("name", "modified", "type", "size", "where")
TITLES = {"name": "Name", "modified": "Date modified",
          "type": "Type", "size": "Size", "where": None}
# #28: None = hide the column; "" = visible but blank header
PC_TITLES = {"name": "Name", "modified": "", "type": None,
             "size": "Free space", "where": None}
BIN_TITLES = {"name": "Name", "modified": "Date deleted",
              "type": "Type", "size": "Size",
              "where": "Original location"}

CONFIG_FILE = os.path.join(os.path.expanduser("~"), ".keyfm.json")
OLD_PIN_FILE = os.path.join(os.path.expanduser("~"), ".keyfm_pins.json")

QUICK_ICONS = {"Home": "🏠", "Desktop": "🖥", "Downloads": "📥",
               "Documents": "🗂", "Pictures": "🖼", "Music": "🎵",
               "Videos": "🎬", "Recycle Bin": "🗑"}
