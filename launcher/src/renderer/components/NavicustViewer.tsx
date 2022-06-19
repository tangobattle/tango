import React from "react";
import { useTranslation } from "react-i18next";

import Box from "@mui/material/Box";
import Chip from "@mui/material/Chip";
import Stack from "@mui/material/Stack";
import Table from "@mui/material/Table";
import TableBody from "@mui/material/TableBody";
import TableCell, { tableCellClasses } from "@mui/material/TableCell";
import TableRow from "@mui/material/TableRow";
import { lighten } from "@mui/system/colorManipulator";

import array2d from "../../array2d";
import { NavicustEditor, NavicustProgram } from "../../saveedit";
import { fallbackLng } from "../i18n";

const NAVICUST_COLORS = {
  red: {
    color: "#de1000",
    plusColor: "#bd0000",
  },
  pink: {
    color: "#de8cc6",
    plusColor: "#bd6ba5",
  },
  yellow: {
    color: "#dede00",
    plusColor: "#bdbd00",
  },
  green: {
    color: "#18c600",
    plusColor: "#00a500",
  },
  blue: {
    color: "#2984de",
    plusColor: "#0860b8",
  },
  white: {
    color: "#dedede",
    plusColor: "#bdbdbd",
  },
};

function placementsToArray2D(
  ncps: (NavicustProgram | null)[],
  placements: {
    id: number;
    rot: number;
    row: number;
    col: number;
    compressed: boolean;
  }[]
) {
  const cust = array2d.full(-1, 7, 7);
  for (let idx = 0; idx < placements.length; ++idx) {
    const placement = placements[idx];
    const ncp = ncps[placement.id]!;

    let squares = array2d.from(ncp.squares, 5, 5);
    for (let i = 0; i < placement.rot; ++i) {
      squares = array2d.rot90(squares);
    }

    for (let i = 0; i < squares.nrows; ++i) {
      for (let j = 0; j < squares.ncols; ++j) {
        const i2 = i + placement.row - 2;
        const j2 = j + placement.col - 2;
        if (i2 >= cust.nrows || j2 >= cust.ncols) {
          continue;
        }
        const v = squares[i * squares.ncols + j];
        if (v == 0) {
          continue;
        }
        if (placement.compressed && v != 1) {
          continue;
        }
        cust[i2 * cust.ncols + j2] = idx;
      }
    }
  }
  return cust;
}

const borderWidth = 4;
const borderColor = "#29314a";
const emptyColor = "#105284";

function navicustBackground(romName: string) {
  switch (romName) {
    case "MEGAMAN6_FXXBR6E":
    case "ROCKEXE6_RXXBR6J":
      return "#E78C39";
    case "MEGAMAN6_GXXBR5E":
    case "ROCKEXE6_GXXBR5J":
      return "#08BD73";
  }
  throw `unknown rom name: ${romName}`;
}

function NavicustGrid({
  ncps,
  placements,
  romName,
  commandLine,
  hasOutOfBounds,
}: {
  ncps: (NavicustProgram | null)[];
  placements: {
    id: number;
    variant: number;
    rot: number;
    row: number;
    col: number;
    compressed: boolean;
  }[];
  commandLine: number;
  hasOutOfBounds: boolean;
  romName: string;
}) {
  const grid = React.useMemo(() => {
    const grid = [];
    const arr2d = placementsToArray2D(ncps, placements);
    for (let row = 0; row < arr2d.nrows; row++) {
      grid.push(array2d.row(arr2d, row));
    }
    return grid;
  }, [ncps, placements]);

  const colors = React.useMemo(() => {
    const colors = [];
    for (const placement of placements) {
      const ncp = ncps[placement.id];
      if (ncp == null) {
        console.error("unrecognized ncp:", placement.id);
        continue;
      }
      const color = ncp.colors[placement.variant];
      if (colors.indexOf(color) != -1) {
        continue;
      }
      colors.push(color);
    }
    return colors;
  }, [ncps, placements]);

  return (
    <div
      style={{
        padding: "20px",
        background: navicustBackground(romName),
        display: "inline-block",
        borderRadius: "4px",
        textAlign: "left",
      }}
    >
      <div style={{ marginBottom: `${borderWidth * 2}px` }}>
        <table
          style={{
            display: "inline-block",
            background: borderColor,
            boxSizing: "border-box",
            borderStyle: "solid",
            borderColor,
            borderWidth: `${borderWidth / 4}px`,
            borderSpacing: 0,
            borderCollapse: "separate",
          }}
        >
          <tbody>
            <tr>
              {[...colors.slice(0, 4), null, null, null, null]
                .slice(0, 4)
                .map((color, i) => (
                  <td
                    key={i}
                    style={{
                      borderStyle: "solid",
                      borderColor,
                      boxSizing: "border-box",
                      borderWidth: `${borderWidth / 2}px`,
                      width: `${borderWidth * 8}px`,
                      height: `${borderWidth * 5}px`,
                      padding: 0,
                    }}
                  >
                    <div
                      style={{
                        width: "100%",
                        height: "100%",
                        background:
                          color != null
                            ? NAVICUST_COLORS[
                                color as keyof typeof NAVICUST_COLORS
                              ].plusColor
                            : emptyColor,
                      }}
                    />
                  </td>
                ))}
            </tr>
          </tbody>
        </table>
        <table
          style={{
            display: "inline-block",
            borderStyle: "solid",
            borderColor: "transparent",
            boxSizing: "border-box",
            borderWidth: `${borderWidth / 4}px`,
            borderSpacing: 0,
            borderCollapse: "separate",
          }}
        >
          <tbody>
            <tr>
              {colors.slice(4).map((color, i) => (
                <td
                  key={i}
                  style={{
                    borderStyle: "solid",
                    borderColor: "transparent",
                    boxSizing: "border-box",
                    borderWidth: `${borderWidth / 2}px`,
                    width: `${borderWidth * 8}px`,
                    height: `${borderWidth * 5}px`,
                    padding: 0,
                  }}
                >
                  <div
                    style={{
                      width: "100%",
                      height: "100%",
                      background:
                        color != null
                          ? NAVICUST_COLORS[
                              color as keyof typeof NAVICUST_COLORS
                            ].plusColor
                          : emptyColor,
                    }}
                  />
                </td>
              ))}
            </tr>
          </tbody>
        </table>
      </div>
      <div>
        <div style={{ position: "relative", display: "inline-block" }}>
          <table
            style={{
              background: borderColor,
              borderStyle: "solid",
              borderColor,
              borderWidth: `${borderWidth / 2}px`,
              boxSizing: "border-box",
              borderSpacing: 0,
              borderCollapse: "separate",
              borderRadius: "4px",
            }}
          >
            <tbody>
              {grid.map((row, i) => (
                <tr key={i}>
                  {row.map((placementIdx, j) => {
                    const placement =
                      placementIdx != -1 ? placements[placementIdx] : null;

                    const ncp = placement != null ? ncps[placement.id] : null;
                    const ncpColor =
                      ncp != null
                        ? NAVICUST_COLORS[
                            ncp.colors[
                              placement!.variant
                            ] as keyof typeof NAVICUST_COLORS
                          ]
                        : null;

                    // prettier-ignore
                    const isCorner = hasOutOfBounds && (
                      (i == 0 && j == 0) ||
                      (i == 0 && j == row.length - 1) ||
                      (i == grid.length - 1 && j == 0) ||
                      (i == grid.length - 1 && j == row.length - 1));

                    const background = isCorner
                      ? "transparent"
                      : ncpColor != null
                      ? ncp!.isSolid
                        ? ncpColor.color
                        : `conic-gradient(
                                          from 90deg at ${borderWidth}px ${borderWidth}px,
                                          ${ncpColor.color} 90deg,
                                          ${ncpColor.plusColor} 0
                                      )
                                      calc(100% + ${borderWidth}px / 2) calc(100% + ${borderWidth}px / 2) /
                                      calc(50% + ${borderWidth}px) calc(50% + ${borderWidth}px)`
                      : emptyColor;

                    const softBorders: string[] = [];
                    const hardBorders: string[] = [];

                    if (
                      placementIdx == -1 ||
                      j <= 0 ||
                      grid[i][j - 1] != placementIdx
                    ) {
                      hardBorders.push(
                        `inset ${borderWidth / 2}px 0 ${borderColor}`
                      );
                    } else {
                      softBorders.push(
                        `inset ${borderWidth / 2}px 0 ${ncpColor!.plusColor}`
                      );
                    }

                    if (
                      placementIdx == -1 ||
                      j >= grid.length - 1 ||
                      grid[i][j + 1] != placementIdx
                    ) {
                      hardBorders.push(
                        `inset ${-borderWidth / 2}px 0 ${borderColor}`
                      );
                    } else {
                      softBorders.push(
                        `inset ${-borderWidth / 2}px 0 ${ncpColor!.plusColor}`
                      );
                    }

                    if (
                      placementIdx == -1 ||
                      i <= 0 ||
                      grid[i - 1][j] != placementIdx
                    ) {
                      hardBorders.push(
                        `inset 0 ${borderWidth / 2}px ${borderColor}`
                      );
                    } else {
                      softBorders.push(
                        `inset 0 ${borderWidth / 2}px ${ncpColor!.plusColor}`
                      );
                    }

                    if (
                      placementIdx == -1 ||
                      i >= row.length - 1 ||
                      grid[i + 1][j] != placementIdx
                    ) {
                      hardBorders.push(
                        `inset 0 ${-borderWidth / 2}px ${borderColor}`
                      );
                    } else {
                      softBorders.push(
                        `inset 0 ${-borderWidth / 2}px ${ncpColor!.plusColor}`
                      );
                    }

                    return (
                      <td
                        style={{
                          width: `${borderWidth * 9}px`,
                          height: `${borderWidth * 9}px`,
                          padding: 0,
                          opacity:
                            hasOutOfBounds &&
                            (i == 0 ||
                              i == grid.length - 1 ||
                              j == 0 ||
                              j == row.length - 1)
                              ? 0.25
                              : 1.0,
                        }}
                        key={j}
                      >
                        <div
                          style={{
                            width: "100%",
                            height: "100%",
                            boxShadow: [...hardBorders, ...softBorders].join(
                              ","
                            ),
                            background,
                          }}
                        />
                      </td>
                    );
                  })}
                </tr>
              ))}
            </tbody>
          </table>
          <hr
            style={{
              top: `${borderWidth * commandLine * 9 + borderWidth * 2.5}px`,
              margin: 0,
              padding: 0,
              position: "absolute",
              width: "100%",
              borderColor,
              borderLeftStyle: "none",
              borderRightStyle: "none",
              borderTopStyle: "none",
              borderBottomStyle: "solid",
              boxSizing: "border-box",
              borderWidth: `${borderWidth}px`,
              pointerEvents: "none",
            }}
          />
          <hr
            style={{
              top: `${
                borderWidth * commandLine * 9 +
                borderWidth * 7 -
                borderWidth / 2
              }px`,
              margin: 0,
              padding: 0,
              position: "absolute",
              width: "100%",
              borderColor,
              borderLeftStyle: "none",
              borderRightStyle: "none",
              borderTopStyle: "solid",
              borderBottomStyle: "none",
              boxSizing: "border-box",
              borderWidth: `${borderWidth}px`,
              pointerEvents: "none",
            }}
          />
        </div>
      </div>
    </div>
  );
}

export default function NavicustViewer({
  editor,
  romName,
  active,
}: {
  editor: NavicustEditor;
  romName: string;
  active: boolean;
}) {
  const { i18n } = useTranslation();
  const placements = React.useMemo(() => {
    const placements = [];
    for (let i = 0; i < 30; i++) {
      const placement = editor.getNavicustBlock(i);
      if (placement == null) {
        continue;
      }
      placements.push(placement);
    }
    return placements;
  }, [editor]);

  const ncps = editor.getNavicustProgramData();

  return (
    <Box
      display={active ? "flex" : "none"}
      flexGrow={1}
      sx={{
        justifyContent: "center",
        py: 1,
        overflow: "auto",
        height: 0,
      }}
    >
      <Stack direction="column" spacing={1}>
        <Box
          sx={{
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
          }}
        >
          <NavicustGrid
            ncps={ncps}
            placements={placements}
            commandLine={editor.getCommandLine()}
            hasOutOfBounds={editor.hasOutOfBounds()}
            romName={romName}
          />
        </Box>
        <Table
          size="small"
          sx={{
            [`& .${tableCellClasses.root}`]: { borderBottom: "none" },
            alignSelf: "center",
            width: "400px",
          }}
        >
          <TableBody>
            <TableRow>
              <TableCell sx={{ verticalAlign: "top", width: "25%" }}>
                <Stack spacing={0.5} flexGrow={1}>
                  {placements.flatMap((placement, i) => {
                    const ncp = ncps[placement.id]!;
                    if (!ncp.isSolid) {
                      return [];
                    }
                    return [
                      <Chip
                        key={i}
                        size="small"
                        sx={{
                          fontSize: "0.9rem",
                          justifyContent: "flex-start",
                          color: "black",
                          backgroundColor: lighten(
                            NAVICUST_COLORS[
                              ncp.colors[
                                placement.variant
                              ] as keyof typeof NAVICUST_COLORS
                            ].color,
                            0.25
                          ),
                        }}
                        label={
                          ncp.name[
                            i18n.resolvedLanguage as keyof typeof ncp.name
                          ] || ncp.name[fallbackLng as keyof typeof ncp.name]
                        }
                      />,
                    ];
                  })}
                </Stack>
              </TableCell>
              <TableCell sx={{ verticalAlign: "top", width: "25%" }}>
                <Stack spacing={0.5} flexGrow={1}>
                  {placements.flatMap((placement, i) => {
                    const ncp = ncps[placement.id]!;
                    if (ncp.isSolid) {
                      return [];
                    }
                    return [
                      <Chip
                        key={i}
                        size="small"
                        sx={{
                          fontSize: "0.9rem",
                          justifyContent: "flex-start",
                          color: "black",
                          backgroundColor: lighten(
                            NAVICUST_COLORS[
                              ncp.colors[
                                placement.variant
                              ] as keyof typeof NAVICUST_COLORS
                            ].color,
                            0.25
                          ),
                        }}
                        label={
                          ncp.name[
                            i18n.resolvedLanguage as keyof typeof ncp.name
                          ] || ncp.name[fallbackLng as keyof typeof ncp.name]
                        }
                      />,
                    ];
                  })}
                </Stack>
              </TableCell>
            </TableRow>
          </TableBody>
        </Table>
      </Stack>
    </Box>
  );
}
