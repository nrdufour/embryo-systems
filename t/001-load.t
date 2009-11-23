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
	etap_can:loaded_ok(embryosys_util, "Module 'embryosys_util' loaded"),
	etap_can:loaded_ok(embryosys, "Module 'embryosys' loaded"),
	etap_can:loaded_ok(embryosys_sup, "Module 'embryosys_sup' loaded"),
	etap:end_tests(),
	ok.
