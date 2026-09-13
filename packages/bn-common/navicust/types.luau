--!strict
export type Placement = {id: number, col: number, row: number, rot: number, compressed: boolean}
export type Bitmap = {size: number, cells: {boolean}}
export type Info = {
    name: string?, description: string?, color: string?, solid: boolean,
    bitmap: (boolean) -> Bitmap?,
}
export type Layout = {cols: number, rows: number, command_line: number, out_of_bounds: boolean, background: Color}
export type Assets = {num_parts: number, part: (number) -> Info?, layout: Layout}
export type View = {
    count: () -> number, part: (number) -> Placement?,
    set_part: (number, Placement?) -> boolean, rebuild: () -> (),
    color_bar: () -> {string},
}
export type Cell = {col: number, row: number}
export type Held = {id: number, rot: number, compressed: boolean, grab_col: number, grab_row: number}
return {}
