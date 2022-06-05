import React from "react";
import { Trans } from "react-i18next";

import BrowserNotSupportedIcon from "@mui/icons-material/BrowserNotSupported";
import Box from "@mui/material/Box";
import Stack from "@mui/material/Stack";
import Tab from "@mui/material/Tab";
import Tabs from "@mui/material/Tabs";
import Typography from "@mui/material/Typography";

import { Editor } from "../../saveedit";
import FolderViewer from "./FolderViewer";
// import ModcardsViewer from "./ModcardsViewer";
import NavicustViewer from "./NavicustViewer";

export default function SaveViewer({ editor }: { editor: Editor }) {
  const navicustEditor = editor.getNavicustEditor();
  const folderEditor = editor.getFolderEditor();

  const availableTabs = React.useMemo(
    () => [
      ...(navicustEditor != null ? ["navicust"] : []),
      ...(folderEditor != null ? ["folder"] : []),
    ],
    [navicustEditor, folderEditor]
  );

  const [tab, setTab] = React.useState("navicust");

  React.useEffect(() => {
    if (availableTabs.indexOf(tab) == -1) {
      setTab(availableTabs[0] || "navicust");
    }
  }, [tab, availableTabs]);

  return (
    <>
      {availableTabs.length > 0 ? (
        <Stack flexGrow={1} flexShrink={0}>
          <Tabs
            sx={{ px: 1 }}
            value={tab}
            onChange={(e, value) => {
              setTab(value);
            }}
          >
            {navicustEditor != null ? (
              <Tab
                label={<Trans i18nKey="play:tab.navicust" />}
                value="navicust"
              />
            ) : null}
            {folderEditor != null ? (
              <Tab label={<Trans i18nKey="play:tab.folder" />} value="folder" />
            ) : null}
          </Tabs>
          {navicustEditor != null ? (
            <NavicustViewer
              gameFamily={editor.getGameFamily()}
              gameVersion={editor.getGameInfo().version}
              editor={navicustEditor}
              active={tab == "navicust"}
            />
          ) : null}
          {folderEditor != null ? (
            <FolderViewer
              gameFamily={editor.getGameFamily()}
              editor={folderEditor}
              active={tab == "folder"}
            />
          ) : null}
          {/* {editor.supportsModcards() ? (
            <ModcardsViewer editor={editor} active={tab == "modcards"} />
          ) : null} */}
        </Stack>
      ) : (
        <Box
          flexGrow={1}
          display="flex"
          justifyContent="center"
          alignItems="center"
          sx={{ userSelect: "none", color: "text.disabled" }}
        >
          <Stack alignItems="center" spacing={1}>
            <BrowserNotSupportedIcon sx={{ fontSize: "4rem" }} />
            <Typography variant="h6">
              <Trans i18nKey="play:save-viewer-unsupported" />
            </Typography>
          </Stack>
        </Box>
      )}
    </>
  );
}
