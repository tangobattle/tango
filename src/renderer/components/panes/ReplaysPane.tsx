import React from "react";
import AutoSizer from "react-virtualized-auto-sizer";
import { FixedSizeList, ListChildComponentProps } from "react-window";

import PlayArrowIcon from "@mui/icons-material/PlayArrow";
import VideoFileIcon from "@mui/icons-material/VideoFile";
import Box from "@mui/material/Box";
import IconButton from "@mui/material/IconButton";
import ListItem from "@mui/material/ListItem";
import ListItemText from "@mui/material/ListItemText";
import Stack from "@mui/material/Stack";

function ReplayItem({ index, style }: ListChildComponentProps) {
  return (
    <ListItem
      style={style}
      key={index}
      secondaryAction={
        <Stack spacing={1} direction="row">
          <IconButton edge="end">
            <VideoFileIcon />
          </IconButton>
          <IconButton edge="end">
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
