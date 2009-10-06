#!/usr/bin/env escript
%% -*- erlang -*-
%%! -pa ./ebin

main(_) ->
    etap:plan(5),

    etap_can:loaded_ok(adtm_util, "Module 'adtm_util' loaded"),

    etap:is(adtm_util:new_state_after(create, none), alive, "Create/None -> Alive"),
    etap:is(adtm_util:new_state_after(hibern, alive), frozen, "Hibern/Alive -> Frozen"),
    etap:is(adtm_util:new_state_after(awake, frozen), alive, "Awake/Frozen -> Alive"),
    etap:is(adtm_util:new_state_after(destroy, alive), destroyed, "Destroy/Alive -> Destroyed"),

    etap:end_tests(),
    ok.
