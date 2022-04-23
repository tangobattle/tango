import React from "react";
import IconButton from "@mui/material/IconButton";
import ContentCopyIcon from "@mui/icons-material/ContentCopy";
import { clipboard } from "@electron/remote";

export function CopyButton({
  value,
  disabled,
}: {
  value: string;
  disabled?: boolean;
}) {
  return (
    <IconButton
      onClick={() => {
        clipboard.writeText(value);
      }}
      edge="end"
      disabled={disabled}
    >
      <ContentCopyIcon fontSize="small" />
    </IconButton>
  );
}
