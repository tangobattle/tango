import path from "path";
import React from "react";

import { app } from "@electron/remote";
import Box from "@mui/material/Box";
import Modal from "@mui/material/Modal";
import Stack from "@mui/material/Stack";
import Typography from "@mui/material/Typography";

import { getReplaysPath } from "../../paths";
import { spawn } from "../../process";
import { ReplayInfo } from "../../replay";
import { Editor } from "../../saveedit/bn6";
import SaveViewer from "./SaveViewer";

export default function ReplayInfoDialog({
  filename,
  replayInfo,
  onClose,
}: {
  filename: string;
  replayInfo: ReplayInfo;
  onClose: () => void;
}) {
  const [editor, setEditor] = React.useState<Editor | null>(null);

  React.useEffect(() => {
    (async () => {
      const proc = await spawn(app, "replaydump", [
        path.join(getReplaysPath(app), filename),
        "dump-ewram",
      ]);

      (async () => {
        for await (const buf of proc.stderr) {
          // eslint-disable-next-line no-console
          console.log(buf.toString());
        }
      })();

      const bufs = [];
      for await (const buf of proc.stdout) {
        bufs.push(buf);
      }

      const buf = Buffer.concat(bufs);
      setEditor(new Editor(new Uint8Array(buf).buffer, replayInfo.rom, false));
    })();
  }, [filename, replayInfo]);

  return (
    <Modal
      open={true}
      onClose={(_e, _reason) => {
        onClose();
      }}
    >
      <Box
        sx={{
          position: "absolute",
          top: "50%",
          left: "50%",
          transform: "translate(-50%, -50%)",
        }}
      >
        <Stack
          sx={{
            width: 600,
            height: 600,
            bgcolor: "background.paper",
            boxShadow: 24,
          }}
          direction="column"
        >
          <Typography
            variant="h6"
            component="h2"
            flexGrow={0}
            flexShrink={0}
            sx={{ px: 2, pt: 1 }}
          >
            {filename}
          </Typography>
          <Box flexGrow={1} sx={{ display: "flex" }}>
            {editor != null ? <SaveViewer editor={editor} /> : null}
          </Box>
        </Stack>
      </Box>
    </Modal>
  );
}
