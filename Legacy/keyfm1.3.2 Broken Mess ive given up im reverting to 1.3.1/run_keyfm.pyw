import os
import sys

here = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, here)

from keyfm.__main__ import main
main()