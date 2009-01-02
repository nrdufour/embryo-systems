/*
 * Embryo Systems
 *
 * $Id$
 *
 * Copyright 2009 Nicolas R. Dufour <nrdufour@gmail.com>
 */
package info.embryosys.adt.manager.impl;

import info.embryosys.adt.manager.AdtManager;
import info.embryosys.adt.manager.Workspace;
import info.embryosys.adt.request.AdtRequest;
import info.embryosys.adt.storage.impl.DummyStorage;

import java.util.LinkedList;
import java.util.List;

/**
 * @author nrdufour
 * 
 */
public class SimpleAdtManager implements AdtManager {

	private List<Workspace> workspaces;

	/**
	 * 
	 */
	public SimpleAdtManager() {
		this.workspaces = new LinkedList<Workspace>();
	}

	/* (non-Javadoc)
	 * @see info.embryosys.adt.manager.AdtManager#init()
	 */
	public void init() {
		// FIXME for now it's hardcoded .. I will find a way to configure that
		// through a conf file

		Workspace dummy = new Workspace("default", new DummyStorage());
		workspaces.add(dummy);
	}

	/* (non-Javadoc)
	 * @see info.embryosys.adt.manager.AdtManager#processRequest(info.embryosys.adt.manager.AdtRequest)
	 */
	public void processRequest(final AdtRequest request) {
		System.out.println("Processing request: "+request);
	}
}
