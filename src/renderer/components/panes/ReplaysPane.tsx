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

function ReplayItem({
  ListChildProps: { index, style },
  replayName,
}: {
  ListChildProps: ListChildComponentProps;
  replayName: string;
}) {
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
      <ListItemText primary={replayName} secondary="bottom text" />
    </ListItem>
  );
}

export default function ReplaysPane({ active }: { active: boolean }) {
  const [replayNames, setReplayNames] = React.useState<string[] | null>(null);

  React.useEffect(() => {
    if (!active) {
      setReplayNames(null);
      return;
    }

    (async () => {
      const names = [];
      for await (const entry of walk(getReplaysPath())) {
        names.push(entry);
      }
      names.sort();
      names.reverse();
      setReplayNames(names);
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
      {replayNames != null ? (
        <AutoSizer>
          {({ height, width }) => (
            <FixedSizeList
              height={height}
              width={width}
              itemCount={replayNames.length}
              itemSize={60}
            >
              {(props) => (
                <ReplayItem
                  ListChildProps={props}
                  replayName={replayNames[props.index]}
                />
              )}
            </FixedSizeList>
          )}
        </AutoSizer>
      ) : null}
    </Box>
  );
}
