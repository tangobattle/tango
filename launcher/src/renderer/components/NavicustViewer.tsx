import { NativeImage } from "electron";
import React from "react";
import { Trans, useTranslation } from "react-i18next";

import { clipboard, nativeImage } from "@electron/remote";
import ContentCopyIcon from "@mui/icons-material/ContentCopy";
import Box from "@mui/material/Box";
import Button, { ButtonProps } from "@mui/material/Button";
import Chip from "@mui/material/Chip";
import Stack from "@mui/material/Stack";
import Table from "@mui/material/Table";
import TableBody from "@mui/material/TableBody";
import TableCell, { tableCellClasses } from "@mui/material/TableCell";
import TableRow from "@mui/material/TableRow";
import Tooltip, { TooltipProps } from "@mui/material/Tooltip";
import { lighten } from "@mui/system/colorManipulator";

import array2d, { Array2D } from "../../array2d";
import { NavicustEditor, NavicustProgram } from "../../saveedit";
import { fallbackLng } from "../i18n";
import { CopyButtonWithLabel } from "./CopyButton";

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
  getNavicustProgramInfo: (id: number) => NavicustProgram | null,
  width: number,
  height: number,
  placements: {
    id: number;
    rot: number;
    row: number;
    col: number;
    compressed: boolean;
  }[]
) {
  const cust = array2d.full(-1, width, height);
  for (let idx = 0; idx < placements.length; ++idx) {
    const placement = placements[idx];
    const ncp = getNavicustProgramInfo(placement.id);

    if (ncp == null) {
      continue;
    }

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
    case "MEGAMAN5_TP_BRBE":
    case "ROCKEXE5_TOBBRBJ":
      return "#218CA5";
    case "MEGAMAN5_TC_BRKE":
    case "ROCKEXE5_TOCBRKJ":
      return "#5A5A4A";
    case "MEGAMANBN4BMB4BE":
    case "ROCK_EXE4_BMB4BJ":
      return "#4252AD";
    case "MEGAMANBN4RSB4WE":
    case "ROCK_EXE4_RSB4WJ":
      return "#BD3139";
    case "MEGA_EXE3_BLA3XE":
    case "ROCK_EXE3_BKA3XJ":
      return "#5A5A5A";
    case "MEGA_EXE3_WHA6BE":
    case "ROCKMAN_EXE3A6BJ":
      return "#4A637B";
  }
  throw `unknown rom name: ${romName}`;
}

interface NavicustGridProps {
  getNavicustProgramInfo: (id: number) => NavicustProgram | null;
  placements: {
    id: number;
    variant: number;
    rot: number;
    row: number;
    col: number;
    compressed: boolean;
  }[];
  grid: Array2D<number>;
  commandLine: number;
  hasOutOfBounds: boolean;
  romName: string;
}

const NavicustGrid = React.forwardRef<HTMLDivElement, NavicustGridProps>(
  (
    {
      getNavicustProgramInfo,
      placements,
      grid,
      romName,
      commandLine,
      hasOutOfBounds,
    }: NavicustGridProps,
    ref
  ) => {
    const grid2d = React.useMemo(() => {
      const grid2d = [];
      for (let row = 0; row < grid.nrows; row++) {
        grid2d.push(array2d.row(grid, row));
      }
      return grid2d;
    }, [grid]);

    const colors = React.useMemo(() => {
      const colors = [];
      for (const placement of placements) {
        const ncp = getNavicustProgramInfo(placement.id);
        if (ncp == null) {
          console.error("unrecognized ncp:", placement.id);
          continue;
        }
        const color = ncp.isSolid
          ? ncp.colors[0]
          : ncp.colors[placement.variant];
        if (colors.indexOf(color) != -1) {
          continue;
        }
        colors.push(color);
      }
      return colors;
    }, [getNavicustProgramInfo, placements]);

    return (
      <div
        ref={ref}
        style={{
          boxSizing: "border-box",
          padding: "20px",
          background: navicustBackground(romName),
          display: "inline-block",
          borderRadius: "4px",
          textAlign: "left",
        }}
      >
        <div
          style={{
            display: "flex",
            boxSizing: "border-box",
            marginBottom: `${borderWidth * 2}px`,
          }}
        >
          <table
            style={{
              boxSizing: "border-box",
              background: borderColor,
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
                        width: `${borderWidth * 6}px`,
                        height: `${borderWidth * 4}px`,
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
                      width: `${borderWidth * 6}px`,
                      height: `${borderWidth * 4}px`,
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
          <div style={{ position: "relative" }}>
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
                {grid2d.map((row, i) => (
                  <tr key={i}>
                    {row.map((placementIdx, j) => {
                      const placement =
                        placementIdx != -1 ? placements[placementIdx] : null;

                      const ncp =
                        placement != null
                          ? getNavicustProgramInfo(placement.id)
                          : null;
                      const color =
                        ncp != null
                          ? ((ncp.isSolid
                              ? ncp.colors[0]
                              : ncp.colors[
                                  placement!.variant
                                ]) as keyof typeof NAVICUST_COLORS)
                          : null;

                      const ncpColor =
                        ncp != null ? NAVICUST_COLORS[color!] : null;

                      // prettier-ignore
                      const isCorner = hasOutOfBounds && (
                      (i == 0 && j == 0) ||
                      (i == 0 && j == row.length - 1) ||
                      (i == grid2d.length - 1 && j == 0) ||
                      (i == grid2d.length - 1 && j == row.length - 1));

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
                        grid2d[i][j - 1] != placementIdx
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
                        j >= grid2d.length - 1 ||
                        grid2d[i][j + 1] != placementIdx
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
                        grid2d[i - 1][j] != placementIdx
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
                        grid2d[i + 1][j] != placementIdx
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
                                i == grid2d.length - 1 ||
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
);

const OFF_COLOR = "#bdbdbd";

function CopyNodeImageToClipboardButton({
  nodeRef,
  disabled,
  TooltipProps,
  ...props
}: {
  nodeRef: React.MutableRefObject<HTMLElement | null>;
  disabled?: boolean;
  TooltipProps?: Partial<TooltipProps>;
} & ButtonProps) {
  const [currentTimeout, setCurrentTimeout] =
    React.useState<NodeJS.Timeout | null>(null);
  return (
    <Tooltip
      open={currentTimeout != null}
      title={<Trans i18nKey="common:copied-to-clipboard" />}
      {...TooltipProps}
    >
      <Button
        onClick={() => {
          if (nodeRef.current == null) {
            return;
          }

          (async () => {
            const img = await nodeToNativeImage(nodeRef.current!);
            clipboard.writeImage(img);
            if (currentTimeout != null) {
              clearTimeout(currentTimeout);
            }
            setCurrentTimeout(
              setTimeout(() => {
                setCurrentTimeout(null);
              }, 1000)
            );
          })();
        }}
        startIcon={<ContentCopyIcon />}
        disabled={disabled}
        {...props}
      >
        <Trans i18nKey="common:copy-image-to-clipboard" />
      </Button>
    </Tooltip>
  );
}

function nodeToNativeImage(node: HTMLElement): Promise<NativeImage> {
  return new Promise((resolve) => {
    const img = document.createElement("img");
    const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
    svg.setAttribute("width", `${node.offsetWidth}`);
    svg.setAttribute("height", `${node.offsetHeight}`);
    svg.setAttribute("viewBox", `0 0 ${node.offsetWidth} ${node.offsetHeight}`);

    const foreignObject = document.createElementNS(
      "http://www.w3.org/2000/svg",
      "foreignObject"
    );
    svg.appendChild(foreignObject);
    foreignObject.setAttribute("width", "100%");
    foreignObject.setAttribute("height", "100%");
    foreignObject.appendChild(node.cloneNode(true));

    const xmlSerializer = new XMLSerializer();

    img.src = `data:image/svg+xml;utf8,${xmlSerializer.serializeToString(svg)}`;
    img.onload = () => {
      const canvas = document.createElement("canvas");
      canvas.width = node.offsetWidth;
      canvas.height = node.offsetHeight;

      const ctx = canvas.getContext("2d")!;
      ctx.drawImage(
        img,
        0,
        0,
        canvas.width,
        canvas.height,
        0,
        0,
        canvas.width,
        canvas.height
      );

      resolve(nativeImage.createFromDataURL(canvas.toDataURL()));
    };
  });
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

  const solidBlocks = placements.filter(
    ({ id }) => editor.getNavicustProgramInfo(id)!.isSolid
  );
  const plusBlocks = placements.filter(
    ({ id }) => !editor.getNavicustProgramInfo(id)!.isSolid
  );

  const grid = placementsToArray2D(
    editor.getNavicustProgramInfo.bind(editor),
    editor.getWidth(),
    editor.getHeight(),
    placements
  );
  const commandLinePlacements = array2d.row(grid, editor.getCommandLine());

  const navicustGridRef = React.useRef<HTMLDivElement | null>(null);

  return (
    <Box display={active ? "flex" : "none"} flexGrow={1}>
      <Stack sx={{ flexGrow: 1 }}>
        <Box sx={{ overflow: "auto", height: 0, flexGrow: 1, px: 1 }}>
          <Stack direction="column" spacing={1}>
            <Box
              sx={{
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
              }}
            >
              <NavicustGrid
                ref={navicustGridRef}
                getNavicustProgramInfo={editor.getNavicustProgramInfo.bind(
                  editor
                )}
                grid={grid}
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
                        const ncp = editor.getNavicustProgramInfo(
                          placement.id
                        )!;
                        if (!ncp.isSolid) {
                          return [];
                        }
                        const name =
                          ncp.name[
                            i18n.resolvedLanguage as keyof typeof ncp.name
                          ] || ncp.name[fallbackLng as keyof typeof ncp.name];

                        const color = (
                          ncp.isSolid
                            ? ncp.colors[0]
                            : ncp.colors[placement.variant]
                        ) as keyof typeof NAVICUST_COLORS;

                        const isActive = commandLinePlacements.indexOf(i) != -1;

                        return [
                          <Chip
                            key={i}
                            size="small"
                            sx={{
                              fontSize: "0.9rem",
                              justifyContent: "flex-start",
                              color: "black",
                              backgroundColor: isActive
                                ? lighten(NAVICUST_COLORS[color].color, 0.25)
                                : OFF_COLOR,
                            }}
                            label={isActive ? name : <del>{name}</del>}
                          />,
                        ];
                      })}
                    </Stack>
                  </TableCell>
                  <TableCell sx={{ verticalAlign: "top", width: "25%" }}>
                    <Stack spacing={0.5} flexGrow={1}>
                      {placements.flatMap((placement, i) => {
                        const ncp = editor.getNavicustProgramInfo(
                          placement.id
                        )!;
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
                              ] ||
                              ncp.name[fallbackLng as keyof typeof ncp.name]
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
        <Box>
          <Stack
            flexGrow={0}
            flexShrink={0}
            direction="row"
            justifyContent="flex-end"
            spacing={1}
            sx={{ px: 1, mb: 0, pt: 1 }}
          >
            <CopyNodeImageToClipboardButton
              nodeRef={navicustGridRef}
              TooltipProps={{ placement: "top" }}
            />
            <CopyButtonWithLabel
              value={(() => {
                const lines = [];
                for (
                  let i = 0;
                  i < Math.max(solidBlocks.length, plusBlocks.length);
                  ++i
                ) {
                  const leftNCP = editor.getNavicustProgramInfo(
                    solidBlocks[i].id
                  )!;
                  const left =
                    i < solidBlocks.length
                      ? leftNCP.name[i18n.resolvedLanguage] ||
                        leftNCP.name[fallbackLng]
                      : "";

                  const rightNCP = editor.getNavicustProgramInfo(
                    plusBlocks[i].id
                  )!;
                  const right =
                    i < plusBlocks.length
                      ? rightNCP.name[i18n.resolvedLanguage] ||
                        rightNCP.name[fallbackLng]
                      : "";
                  lines.push(left + "\t" + right);
                }
                return lines.join("\n");
              })()}
              TooltipProps={{ placement: "top" }}
            />
          </Stack>
        </Box>
      </Stack>
    </Box>
  );
}
