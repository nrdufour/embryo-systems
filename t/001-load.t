#!/usr/bin/env escript
%% -*- erlang -*-
%%! -pa ./ebin

main(_) ->
    etap:plan(2),
    etap_can:loaded_ok(adtm_server, "Module 'adtm_server' loaded"),
    etap_can:can_ok(adtm_server, server_info),
    etap:end_tests(),
    ok.
