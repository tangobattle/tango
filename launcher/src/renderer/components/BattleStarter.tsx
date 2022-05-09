import { readFile } from "fs/promises";
import path from "path";
import React from "react";
import { Trans, useTranslation } from "react-i18next";
import semver from "semver";

import { app, shell } from "@electron/remote";
import CheckIcon from "@mui/icons-material/Check";
import CloseIcon from "@mui/icons-material/Close";
import FolderOpenIcon from "@mui/icons-material/FolderOpen";
import KeyboardArrowDownIcon from "@mui/icons-material/KeyboardArrowDown";
import KeyboardArrowUpIcon from "@mui/icons-material/KeyboardArrowUp";
import PlayArrowIcon from "@mui/icons-material/PlayArrow";
import SportsMmaIcon from "@mui/icons-material/SportsMma";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import Checkbox from "@mui/material/Checkbox";
import CircularProgress from "@mui/material/CircularProgress";
import Collapse from "@mui/material/Collapse";
import Divider from "@mui/material/Divider";
import FormControl from "@mui/material/FormControl";
import FormControlLabel from "@mui/material/FormControlLabel";
import FormGroup from "@mui/material/FormGroup";
import IconButton from "@mui/material/IconButton";
import InputLabel from "@mui/material/InputLabel";
import ListItemText from "@mui/material/ListItemText";
import ListSubheader from "@mui/material/ListSubheader";
import MenuItem from "@mui/material/MenuItem";
import Select from "@mui/material/Select";
import Stack from "@mui/material/Stack";
import Table from "@mui/material/Table";
import TableBody from "@mui/material/TableBody";
import TableCell from "@mui/material/TableCell";
import TableHead from "@mui/material/TableHead";
import TableRow from "@mui/material/TableRow";
import TextField from "@mui/material/TextField";
import Tooltip from "@mui/material/Tooltip";
import Typography from "@mui/material/Typography";

import { getBasePath, getSavesPath } from "../../paths";
import { ToCoreMessage_StartRequest_MatchSettings } from "../../protos/ipc";
import { KNOWN_ROMS } from "../../rom";
import { Editor } from "../../saveedit/bn6";
import { CoreSupervisor } from "./CoreSupervisor";
import { usePatches } from "./PatchesContext";
import { useROMs } from "./ROMsContext";
import { useSaves } from "./SavesContext";
import SaveViewer from "./SaveViewer";

const MATCH_TYPES = ["single", "triple"];

export default function BattleStarter() {
  const [linkCode, setLinkCode] = React.useState("");
  const [matchSettings, setMatchSettings] =
    React.useState<ToCoreMessage_StartRequest_MatchSettings | null>({});
  const [startedState, setStartedState] = React.useState<{
    linkCode: string | null;
  } | null>(null);
  const [incarnation, setIncarnation] = React.useState(0);
  const [uncollapsed, setUncollapsed] = React.useState(false);

  return (
    <Stack>
      <Collapse in={uncollapsed}>
        <Divider />
        <Box
          sx={{
            px: 1,
            pb: 1,
          }}
        >
          <Table size="small">
            <TableHead>
              <TableRow>
                <TableCell></TableCell>
                <TableCell sx={{ width: "30%", fontWeight: "bold" }}>
                  You
                </TableCell>
                <TableCell sx={{ width: "30%", fontWeight: "bold" }}>
                  Opponent
                </TableCell>
              </TableRow>
            </TableHead>
            <TableBody>
              <TableRow>
                <TableCell component="th" sx={{ fontWeight: "bold" }}>
                  Game
                </TableCell>
                <TableCell>Mega Man Battle Network 4: Blue Moon</TableCell>
                <TableCell>Mega Man Battle Network 4: Red Sun</TableCell>
              </TableRow>
              <TableRow>
                <TableCell component="th" sx={{ fontWeight: "bold" }}>
                  Match type
                </TableCell>
                <TableCell>
                  <Select
                    variant="standard"
                    size="small"
                    value={matchSettings!.matchType}
                    onChange={(e) => {
                      setMatchSettings((ms) => ({
                        ...(ms as any),
                        matchType: e.target.value,
                      }));
                    }}
                    disabled={false}
                  >
                    {MATCH_TYPES.map((v, k) => (
                      <MenuItem key={k} value={k}>
                        {k == 0 ? (
                          <Trans i18nKey="play:match-type.single" />
                        ) : k == 1 ? (
                          <Trans i18nKey="play:match-type.triple" />
                        ) : null}
                      </MenuItem>
                    ))}
                  </Select>
                </TableCell>
                <TableCell>Triple</TableCell>
              </TableRow>
              <TableRow>
                <TableCell component="th" sx={{ fontWeight: "bold" }}>
                  Input delay
                </TableCell>
                <TableCell>
                  <TextField
                    variant="standard"
                    type="number"
                    InputProps={{ inputProps: { min: 3, max: 10 } }}
                  />
                </TableCell>
                <TableCell>3</TableCell>
              </TableRow>
            </TableBody>
          </Table>
        </Box>
      </Collapse>
      <Stack
        flexGrow={0}
        flexShrink={0}
        direction="row"
        justifyContent="flex-end"
        spacing={1}
        sx={{ px: 1, mb: 0 }}
        component="form"
        onSubmit={(e: any) => {
          e.preventDefault();
          setUncollapsed(true);
        }}
      >
        <Box flexGrow={1} flexShrink={0}>
          <TextField
            disabled={uncollapsed}
            size="small"
            label={<Trans i18nKey={"play:link-code"} />}
            value={linkCode}
            onChange={(e) => {
              setLinkCode(
                e.target.value
                  .toLowerCase()
                  .replace(/[^a-z0-9]/g, "")
                  .slice(0, 40)
              );
            }}
            InputProps={{
              endAdornment: uncollapsed ? (
                <CircularProgress size="1rem" color="inherit" />
              ) : null,
            }}
            fullWidth
          />
        </Box>

        {!uncollapsed ? (
          <Button
            type="submit"
            variant="contained"
            startIcon={linkCode != "" ? <SportsMmaIcon /> : <PlayArrowIcon />}
            disabled={false}
          >
            {linkCode != "" ? (
              <Trans i18nKey="play:fight" />
            ) : (
              <Trans i18nKey="play:play" />
            )}
          </Button>
        ) : (
          <>
            <FormGroup>
              <FormControlLabel
                control={<Checkbox />}
                label={<Trans i18nKey={"play:ready"} />}
              />
            </FormGroup>
            <Button
              color="error"
              variant="contained"
              startIcon={<CloseIcon />}
              disabled={false}
            >
              <Trans i18nKey="play:cancel" />
            </Button>
          </>
        )}
      </Stack>
    </Stack>
  );
}
