#!/usr/bin/env escript
%% -*- erlang -*-
%%! -pa ./ebin

main(_) ->
    etap:plan(2),
    etap_can:loaded_ok(adt_server, "Module 'adt_server' loaded"),
    etap_can:can_ok(adt_server, server_info),
    etap:end_tests(),
    ok.
