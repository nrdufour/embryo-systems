#!/usr/bin/env escript
%% -*- erlang -*-
%%! -pa ./ebin

main(_) ->
    etap:plan(unknown),
    etap_can:loaded_ok(adtm_class, "Module 'adtm_class' loaded"),
    etap_can:loaded_ok(adtm_attribute, "Module 'adtm_attribute' loaded"),
    etap_can:loaded_ok(adtm_link, "Module 'adtm_link' loaded"),
    etap_can:loaded_ok(adtm_object, "Module 'adtm_object' loaded"),
    etap_can:loaded_ok(adtm_server, "Module 'adtm_server' loaded"),
    etap_can:loaded_ok(adtm_storage_server, "Module 'adtm_storage_server' loaded"),
    etap_can:loaded_ok(adtm_util, "Module 'adtm_util' loaded"),
    etap_can:loaded_ok(adtm, "Module 'adtm' loaded"),
    etap_can:loaded_ok(adtm_sup, "Module 'adtm_sup' loaded"),
    etap:end_tests(),
    ok.
