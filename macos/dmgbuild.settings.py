format = "UDBZ"
files = ["Tango.app"]

badge_icon = "Tango.app/Contents/Resources/Tango.icns"

symlinks = {"Applications": "/Applications"}
icon_locations = {"Tango.app": (140, 120), "Applications": (500, 120)}

background = "builtin-arrow"

show_status_bar = False
show_tab_view = False
show_toolbar = False
show_pathbar = False
show_sidebar = False
sidebar_width = 180

window_rect = ((100, 100), (640, 280))

default_view = "icon-view"

show_icon_preview = False

include_icon_view_settings = "auto"
include_list_view_settings = "auto"

arrange_by = None
grid_offset = (0, 0)
grid_spacing = 100
scroll_position = (0, 0)
label_pos = "bottom"  # or 'right'
text_size = 16
icon_size = 128

list_icon_size = 16
list_text_size = 12
list_scroll_position = (0, 0)
list_sort_by = "name"
list_use_relative_dates = True
list_calculate_all_sizes = (False,)
list_columns = ("name", "date-modified", "size", "kind", "date-added")
list_column_widths = {
    "name": 300,
    "date-modified": 181,
    "date-created": 181,
    "date-added": 181,
    "date-last-opened": 181,
    "size": 97,
    "kind": 115,
    "label": 100,
    "version": 75,
    "comments": 300,
}
list_column_sort_directions = {
    "name": "ascending",
    "date-modified": "descending",
    "date-created": "descending",
    "date-added": "descending",
    "date-last-opened": "descending",
    "size": "descending",
    "kind": "ascending",
    "label": "ascending",
    "version": "ascending",
    "comments": "ascending",
}
