export type U32=number;
export type U16=number;
export type Keymapping={"up":string;"down":string;"left":string;"right":string;"a":string;"b":string;"l":string;"r":string;"select":string;"start":string;};
export type Args={"rom_path":string;"save_path":string;"session_id":string;"input_delay":U32;"match_type":U16;"replay_prefix":string;"matchmaking_connect_addr":string;"ice_servers":(string)[];"keymapping":Keymapping;};
export type Notification=("Running"|"Waiting"|"Connecting"|"Done");
