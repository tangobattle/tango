import { opendir } from "fs/promises";
import path from "path";
import React from "react";
import AutoSizer from "react-virtualized-auto-sizer";
import { FixedSizeList, ListChildComponentProps } from "react-window";

import FolderIcon from "@mui/icons-material/Folder";
import PlayArrowIcon from "@mui/icons-material/PlayArrow";
import VideoFileIcon from "@mui/icons-material/VideoFile";
import Box from "@mui/material/Box";
import IconButton from "@mui/material/IconButton";
import ListItem from "@mui/material/ListItem";
import ListItemText from "@mui/material/ListItemText";
import Stack from "@mui/material/Stack";

import { getReplaysPath } from "../../../paths";

async function* walk(dir: string, root?: string): AsyncIterable<string> {
  if (root == null) {
    root = dir;
  }
  for await (const d of await opendir(dir)) {
    const entry = path.join(dir, d.name);
    if (d.isDirectory()) {
      yield* await walk(entry, root);
    } else if (d.isFile()) {
      yield path.relative(root, entry);
    }
  }
}

function ReplayItem({ index, style }: ListChildComponentProps) {
  return (
    <ListItem
      style={style}
      key={index}
      sx={{ userSelect: "none" }}
      secondaryAction={
        <Stack direction="row">
          <IconButton>
            <FolderIcon />
          </IconButton>
          <IconButton>
            <VideoFileIcon />
          </IconButton>
          <IconButton>
            <PlayArrowIcon />
          </IconButton>
        </Stack>
      }
    >
      <ListItemText primary="Single-line item" secondary="bottom text" />
    </ListItem>
  );
}

export default function ReplaysPane({ active }: { active: boolean }) {
  React.useEffect(() => {
    (async () => {
      for await (const entry of walk(getReplaysPath())) {
        console.log(entry);
      }
    })();
  }, [active]);

  return (
    <Box
      sx={{
        width: "100%",
        height: "100%",
        display: active ? "block" : "none",
      }}
    >
      <AutoSizer>
        {({ height, width }) => (
          <FixedSizeList
            height={height}
            width={width}
            itemCount={200}
            itemSize={60}
          >
            {ReplayItem}
          </FixedSizeList>
        )}
      </AutoSizer>
    </Box>
  );
}
