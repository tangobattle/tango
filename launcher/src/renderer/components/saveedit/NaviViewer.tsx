import React from "react";

import Box from "@mui/material/Box";
import Stack from "@mui/material/Stack";
import Typography from "@mui/material/Typography";

import { NaviEditor } from "../../../saveedit";

export default function NaviViewer({
  editor,
  active,
}: {
  editor: NaviEditor;
  active: boolean;
}) {
  const naviInfo = editor.getNaviInfo(editor.getNavi());

  const canvasRef = React.useRef<HTMLCanvasElement | null>(null);
  React.useEffect(() => {
    const ctx = canvasRef.current!.getContext("2d")!;
    ctx.clearRect(0, 0, canvasRef.current!.width, canvasRef.current!.height);
    ctx.putImageData(naviInfo.emblem, -1, 0);
  }, [naviInfo]);

  return (
    <Box display={active ? "flex" : "none"} flexGrow={1}>
      <Stack sx={{ flexGrow: 1 }}>
        <Box
          sx={{
            display: "flex",
            overflow: "auto",
            height: 0,
            flexGrow: 1,
            px: 1,
          }}
        >
          <Stack
            direction="column"
            spacing={1}
            sx={{
              flexGrow: 1,
              alignItems: "center",
              justifyContent: "center",
            }}
          >
            <canvas
              width={15}
              height={15}
              style={{
                width: "45px",
                height: "45px",
                imageRendering: "pixelated",
              }}
              ref={canvasRef}
            />
            <Typography variant="h6">{naviInfo.name}</Typography>
          </Stack>
        </Box>
      </Stack>
    </Box>
  );
}
